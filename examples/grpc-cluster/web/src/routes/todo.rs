//! The todo resource: what it looks like over HTTP, and the five routes.
//!
//! The DTOs sit beside the handlers rather than in a `dto` module of their own:
//! a wire shape and the handler that returns it change together, and splitting
//! them puts a file boundary between two edits that are always one edit.

use axum::{
    Json, Router,
    extract::{Path, State},
    routing::{get, post},
};
use example_todo::proto::{
    CompleteTodoRequest, CreateTodoRequest, DeleteTodoRequest, GetTodoRequest, ListTodosRequest,
    todo_service_client::TodoServiceClient,
};
use garde::Validate;
use serde::{Deserialize, Serialize};
use toolbox_web::{ApiError, Authenticated, PageQuery, ValidJson};

use crate::{auth::Admin, routes::from_backend, state::AppState};

/// A todo as the HTTP API presents it.
///
/// A hand-written DTO rather than the proto type: proto3 enums become `i32`,
/// so a generated schema documents them as integers.
#[derive(Debug, Serialize, utoipa::ToSchema)]
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

impl From<example_todo::proto::Todo> for TodoDto {
    fn from(t: example_todo::proto::Todo) -> Self {
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
pub struct NewTodo {
    /// What to do.
    #[garde(length(min = 1, max = 200))]
    pub title: String,
}

/// One page of todos.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct TodoPage {
    /// The rows.
    pub items: Vec<TodoDto>,
    /// How many matched in total.
    pub total: i64,
}

/// Which version the caller believes it is completing.
#[derive(Debug, Deserialize)]
pub struct Complete {
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

/// `GET`: one page of todos.
///
/// # Arguments
///
/// * `state` - The gateway's state, for the backend channel.
/// * `page` - The window and sort, already validated against the maximum limit.
#[utoipa::path(
    get, path = "/api/todos",
    params(("offset" = Option<i64>, Query,), ("limit" = Option<i64>, Query,), ("sort" = Option<String>, Query,)),
    responses((status = 200, body = TodoPage))
)]
pub(crate) async fn list(
    State(state): State<AppState>,
    PageQuery(page): PageQuery,
) -> Result<Json<TodoPage>, ApiError> {
    let response = client(&state)
        .list_todos(ListTodosRequest {
            page: Some(toolbox_grpc::PageRequestProto::from(&page)),
            title_contains: String::new(),
        })
        .await
        .map_err(|s| from_backend(&s))?
        .into_inner();

    Ok(Json(TodoPage {
        items: response.items.into_iter().map(TodoDto::from).collect(),
        total: response.page.map_or(0, |p| p.total),
    }))
}

/// `GET`: one todo.
///
/// # Arguments
///
/// * `state` - The gateway's state, for the backend channel.
/// * `id` - Which todo. A miss is the backend's `TODO_NOT_FOUND`, relayed with
///   its code intact.
#[utoipa::path(
    get, path = "/api/todos/{id}",
    params(("id" = i32, Path,)),
    responses((status = 200, body = TodoDto))
)]
pub(crate) async fn fetch(
    State(state): State<AppState>,
    Path(id): Path<i32>,
) -> Result<Json<TodoDto>, ApiError> {
    let todo = client(&state)
        .get_todo(GetTodoRequest { id })
        .await
        .map_err(|s| from_backend(&s))?
        .into_inner();
    Ok(Json(todo.into()))
}

/// `POST`: add a todo.
///
/// # Arguments
///
/// * `state` - The gateway's state, for the backend channel.
/// * `body` - The new todo, rejected here if invalid so no hop is made.
#[utoipa::path(post, path = "/api/todos", request_body = NewTodo,
    responses((status = 200, body = TodoDto)))]
pub(crate) async fn create(
    State(state): State<AppState>,
    ValidJson(body): ValidJson<NewTodo>,
) -> Result<Json<TodoDto>, ApiError> {
    let todo = client(&state)
        .create_todo(CreateTodoRequest { title: body.title })
        .await
        .map_err(|s| from_backend(&s))?
        .into_inner();
    Ok(Json(todo.into()))
}

/// `POST`: mark a todo done, if nobody changed it first.
///
/// # Arguments
///
/// * `state` - The gateway's state, for the backend channel.
/// * `id` - Which todo.
/// * `body` - The version the caller read.
pub(crate) async fn complete(
    State(state): State<AppState>,
    Path(id): Path<i32>,
    Json(body): Json<Complete>,
) -> Result<Json<TodoDto>, ApiError> {
    let todo = client(&state)
        .complete_todo(CompleteTodoRequest {
            id,
            version: body.version,
        })
        .await
        .map_err(|s| from_backend(&s))?
        .into_inner();
    Ok(Json(todo.into()))
}

/// `DELETE`: soft-delete a todo. Needs the admin role, and the type system
/// checks it.
///
/// # Arguments
///
/// * `_` - The extractor that enforces the role. An anonymous caller is a 401
///   and never reaches the backend.
/// * `state` - The gateway's state, for the backend channel.
/// * `id` - Which todo.
#[utoipa::path(delete, path = "/api/todos/{id}", params(("id" = i32, Path,)),
    responses((status = 200)), security(("bearer" = [])))]
pub(crate) async fn remove(
    _: Authenticated<Admin>,
    State(state): State<AppState>,
    Path(id): Path<i32>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let deleted = client(&state)
        .delete_todo(DeleteTodoRequest { id })
        .await
        .map_err(|s| from_backend(&s))?
        .into_inner()
        .deleted;
    Ok(Json(serde_json::json!({ "deleted": deleted })))
}
