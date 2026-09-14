//! The todo resource: what it looks like over HTTP, and the five routes.
//!
//! The DTOs sit beside the handlers rather than in a `dto` module of their own:
//! a wire shape and the handler that returns it change together, and splitting
//! them puts a file boundary between two edits that are always one edit.

use std::convert::Infallible;

use axum::{
    Json, Router,
    extract::{Path, State},
    http::{StatusCode, header::CONTENT_TYPE},
    response::{
        IntoResponse, Response,
        sse::{Event, Sse},
    },
    routing::{get, post},
};
use futures_core::Stream;
use garde::Validate;
use serde::{Deserialize, Serialize};
use {{crate_name}}_todo::proto::{
    CompleteTodoRequest, CreateTodoRequest, DeleteTodoRequest, GetTodoRequest, ListTodosRequest,
    Todo, todo_service_client::TodoServiceClient,
};
use tokio_stream::{StreamExt as _, wrappers::BroadcastStream};
use toolbox_auth::AssertedPrincipal;
use toolbox_grpc::{client::asserting, with_retry};
use toolbox_web::{
    ApiError, Authenticated, Idempotent, MaybeAuthenticated, PageQuery, ValidJson,
    idempotency::{IdempotencyOutcome, StoredResponse, in_flight_error},
    realtime::{SseConfig, sse_from_events},
};

use crate::{
    auth::Admin,
    routes::from_backend,
    state::{AppState, TODOS_TOPIC},
};

/// A todo as the HTTP API presents it.
///
/// A hand-written DTO rather than the proto type: proto3 enums become `i32`,
/// so a generated schema documents them as integers.
#[derive(Debug, Serialize, Deserialize, utoipa::ToSchema)]
pub struct TodoDto {
    /// Its id.
    pub id: i32,
    /// What to do.
    pub title: String,
    /// Whether it is done.
    pub done: bool,
    /// For optimistic locking on completion.
    pub version: i32,
}

impl From<Todo> for TodoDto {
    fn from(t: Todo) -> Self {
        Self {
            id: t.id,
            title: t.title,
            done: t.done,
            version: t.version,
        }
    }
}

/// A new todo.
#[derive(Debug, Deserialize, Validate, utoipa::ToSchema)]
pub struct NewTodoRequest {
    /// What to do.
    #[garde(length(min = 1, max = 200))]
    pub title: String,
}

/// One page of todos.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct TodoPageResponse {
    /// The rows.
    pub items: Vec<TodoDto>,
    /// How many matched in total.
    pub total: i64,
}

/// Which version the caller believes it is completing.
#[derive(Debug, Deserialize)]
pub struct CompleteRequest {
    /// The version last read. A stale one is a 409, not a silent overwrite.
    pub version: i32,
}

/// The todo routes.
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/todos", get(list).post(create))
        // The role is in the signature, so it is visible right here in the
        // route table rather than buried in a handler body.
        .route("/api/todos/{id}", get(fetch).delete(remove))
        .route("/api/todos/{id}/complete", post(complete))
}

/// The realtime routes, kept apart from [`router`] because they need
/// `realtime_stack` rather than `http_stack` - no timeout, no body limit.
pub fn realtime_router() -> Router<AppState> {
    Router::new().route("/api/todos/events", get(events))
}

/// `GET`: a `todo.*` event every time a todo changes, over SSE.
///
/// The payload is only `{ id }` - `resume_from`'s reconnect story is "re-query
/// the domain for what you missed, then subscribe live", not "replay what
/// arrived while you were gone", so the event never needs to carry the whole
/// row. Left unauthenticated on purpose: an id and a type leak nothing that
/// `GET /api/todos` does not already hand out to anyone.
///
/// # Arguments
///
/// * `state` - The gateway's state, for the hub every change is fanned out on.
pub(crate) async fn events(
    State(state): State<AppState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>> + Send> {
    let rx = state.hub.subscribe(TODOS_TOPIC);
    let stream = BroadcastStream::new(rx).filter_map(Result::ok);
    sse_from_events(stream, SseConfig::default())
}

/// A client for the backend, with the channel's negotiated message limits.
///
/// # Arguments
///
/// * `state` - Read for the channel and its limits. Built per request because
///   a tonic client is a cheap wrapper around a cloned channel.
fn client(state: &AppState) -> TodoServiceClient<toolbox_grpc::ClientService> {
    TodoServiceClient::new(state.todos.channel())
        .max_decoding_message_size(state.todos.limits().max_decoding)
        .max_encoding_message_size(state.todos.limits().max_encoding)
}

/// Run `f` with the caller's principal attached to every backend call it
/// makes, if there is one.
///
/// Anonymous reads still reach the backend with no principal - `asserting`
/// with nothing to attach is exactly a call made outside any scope. What
/// changes is that `TodoService::delete_todo` refuses a caller `identity_layer`
/// never resolved.
///
/// # Arguments
///
/// * `principal` - The caller, if `session_layer` resolved one.
/// * `f` - The backend call to make.
async fn assert_caller<F: Future>(principal: &MaybeAuthenticated, f: F) -> F::Output {
    match &principal.0 {
        Some(p) => asserting(AssertedPrincipal::from(p).encode(), f).await,
        None => f.await,
    }
}

/// `GET`: one page of todos.
///
/// # Arguments
///
/// * `state` - The gateway's state, for the backend channel.
/// * `principal` - The caller, attached to the backend call if there is one.
/// * `page` - The window and sort, already validated against the maximum limit.
#[utoipa::path(
    get, path = "/api/todos",
    params(("offset" = Option<i64>, Query,), ("limit" = Option<i64>, Query,), ("sort" = Option<String>, Query,)),
    responses((status = 200, body = TodoPageResponse))
)]
pub(crate) async fn list(
    State(state): State<AppState>,
    principal: MaybeAuthenticated,
    PageQuery(page): PageQuery,
) -> Result<Json<TodoPageResponse>, ApiError> {
    let request = ListTodosRequest {
        page: Some(toolbox_grpc::PageRequestProto::from(&page)),
        title_contains: String::new(),
    };
    let response = assert_caller(&principal, async {
        with_retry(state.todos.retry(), "ListTodos", || {
            let mut client = client(&state);
            let request = request.clone();
            async move { client.list_todos(request).await }
        })
        .await
    })
    .await
    .map_err(|s| from_backend(&s))?
    .into_inner();

    Ok(Json(TodoPageResponse {
        items: response.items.into_iter().map(TodoDto::from).collect(),
        total: response.page.map_or(0, |p| p.total),
    }))
}

/// `GET`: one todo.
///
/// # Arguments
///
/// * `state` - The gateway's state, for the backend channel.
/// * `principal` - The caller, attached to the backend call if there is one.
/// * `id` - Which todo. A miss is the backend's `TODO_NOT_FOUND`, relayed with
///   its code intact.
#[utoipa::path(
    get, path = "/api/todos/{id}",
    params(("id" = i32, Path,)),
    responses((status = 200, body = TodoDto))
)]
pub(crate) async fn fetch(
    State(state): State<AppState>,
    principal: MaybeAuthenticated,
    Path(id): Path<i32>,
) -> Result<Json<TodoDto>, ApiError> {
    let todo = assert_caller(&principal, async {
        with_retry(state.todos.retry(), "GetTodo", || {
            let mut client = client(&state);
            async move { client.get_todo(GetTodoRequest { id }).await }
        })
        .await
    })
    .await
    .map_err(|s| from_backend(&s))?
    .into_inner();
    Ok(Json(todo.into()))
}

/// `POST`: add a todo.
///
/// A repeated `Idempotency-Key` replays the first response rather than
/// creating a second todo - the point of sending one at all.
///
/// # Arguments
///
/// * `state` - The gateway's state, for the backend channel.
/// * `principal` - The caller, attached to the backend call if there is one.
/// * `key` - The idempotency key, if the caller sent one.
/// * `body` - The new todo, rejected here if invalid so no hop is made.
#[utoipa::path(post, path = "/api/todos", request_body = NewTodoRequest,
    responses((status = 200, body = TodoDto)))]
pub(crate) async fn create(
    State(state): State<AppState>,
    principal: MaybeAuthenticated,
    Idempotent(key): Idempotent,
    ValidJson(body): ValidJson<NewTodoRequest>,
) -> Result<Response, ApiError> {
    let Some(key) = key else {
        let todo = do_create(&state, &principal, body).await?;
        return Ok(Json(todo).into_response());
    };

    match state.idempotency.claim(&key, "create_todo").await? {
        IdempotencyOutcome::InFlight => Err(in_flight_error()),
        IdempotencyOutcome::Replay(stored) => Ok(replay(&stored)),
        IdempotencyOutcome::Fresh => match do_create(&state, &principal, body).await {
            Ok(todo) => {
                let stored = stored_json(&todo)?;
                state
                    .idempotency
                    .record(&key, "create_todo", &stored)
                    .await?;
                Ok(replay(&stored))
            }
            Err(e) => {
                // A failed attempt is not an outcome worth replaying, and
                // leaving the key claimed would make the retry - the entire
                // point of sending one - impossible.
                state.idempotency.release(&key, "create_todo").await?;
                Err(e)
            }
        },
    }
}

/// The actual creation, shared by the keyed and unkeyed paths.
async fn do_create(
    state: &AppState,
    principal: &MaybeAuthenticated,
    body: NewTodoRequest,
) -> Result<TodoDto, ApiError> {
    let todo = assert_caller(principal, async {
        client(state)
            .create_todo(CreateTodoRequest { title: body.title })
            .await
    })
    .await
    .map_err(|s| from_backend(&s))?
    .into_inner();
    Ok(todo.into())
}

/// `POST`: mark a todo done, if nobody changed it first.
///
/// # Arguments
///
/// * `state` - The gateway's state, for the backend channel.
/// * `principal` - The caller, attached to the backend call if there is one.
/// * `id` - Which todo.
/// * `body` - The version the caller read.
pub(crate) async fn complete(
    State(state): State<AppState>,
    principal: MaybeAuthenticated,
    Path(id): Path<i32>,
    Json(body): Json<CompleteRequest>,
) -> Result<Json<TodoDto>, ApiError> {
    let todo = assert_caller(&principal, async {
        client(&state)
            .complete_todo(CompleteTodoRequest {
                id,
                version: body.version,
            })
            .await
    })
    .await
    .map_err(|s| from_backend(&s))?
    .into_inner();
    Ok(Json(todo.into()))
}

/// `DELETE`: soft-delete a todo. Needs the admin role, and the type system
/// checks it.
///
/// The gateway's own gate is not the only one: `TodoService::delete_todo`
/// checks the asserted principal again, because a caller that reached the
/// backend some other way never passed through this handler at all.
///
/// # Arguments
///
/// * `caller` - The extractor that enforces the role. An anonymous caller is a
///   401 and never reaches the backend.
/// * `state` - The gateway's state, for the backend channel.
/// * `id` - Which todo.
#[utoipa::path(delete, path = "/api/todos/{id}", params(("id" = i32, Path,)),
    responses((status = 200)), security(("bearer" = [])))]
pub(crate) async fn remove(
    caller: Authenticated<Admin>,
    State(state): State<AppState>,
    Path(id): Path<i32>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let encoded = AssertedPrincipal::from(caller.principal()).encode();
    let deleted = asserting(encoded, async {
        client(&state).delete_todo(DeleteTodoRequest { id }).await
    })
    .await
    .map_err(|s| from_backend(&s))?
    .into_inner()
    .deleted;
    Ok(Json(serde_json::json!({ "deleted": deleted })))
}

/// Turn a successful response into what an idempotent replay stores.
fn stored_json(todo: &TodoDto) -> Result<StoredResponse, ApiError> {
    Ok(StoredResponse {
        status: StatusCode::OK.as_u16(),
        body: serde_json::to_vec(todo).map_err(ApiError::internal)?,
        content_type: "application/json".to_owned(),
    })
}

/// Rebuild the response a stored record describes.
fn replay(stored: &StoredResponse) -> Response {
    let status = StatusCode::from_u16(stored.status).unwrap_or(StatusCode::OK);
    (
        status,
        [(CONTENT_TYPE, stored.content_type.clone())],
        stored.body.clone(),
    )
        .into_response()
}
