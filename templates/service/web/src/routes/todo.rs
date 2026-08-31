//! The todo resource: what it looks like over HTTP, and its routes.
//!
//! The DTOs sit beside the handlers rather than in a `dto` module of their own:
//! a wire shape and the handler that returns it change together, and splitting
//! them puts a file boundary between two edits that are always one edit.

use axum::{
    Json, Router,
    extract::{Path, State},
    routing::get,
};
use garde::Validate;
use serde::{Deserialize, Serialize};
use {{crate_name}}_todo::proto::{
    CreateTodoRequest, GetTodoRequest, ListTodosRequest, todo_service_client::TodoServiceClient,
};
use toolbox_web::{ApiError, Authenticated, PageQuery, ValidJson};

use crate::{auth::Admin, routes::from_backend, state::AppState};

/// A todo as the HTTP API presents it.
///
/// Hand-written rather than the proto type: proto3 enums serialize as integers,
/// so a generated schema documents them awkwardly.
#[derive(Debug, Serialize)]
pub struct TodoDto {
    /// Its id.
    pub id: i32,
    /// What to do.
    pub title: String,
    /// Whether it is done.
    pub done: bool,
    /// For optimistic locking.
    pub version: i32,
}

impl From<{{crate_name}}_todo::proto::Todo> for TodoDto {
    fn from(t: {{crate_name}}_todo::proto::Todo) -> Self {
        Self {
            id: t.id,
            title: t.title,
            done: t.done,
            version: t.version,
        }
    }
}

/// A new todo.
#[derive(Debug, Deserialize, Validate)]
pub struct NewTodo {
    /// What to do.
    #[garde(length(min = 1, max = 200))]
    pub title: String,
}

/// The todo routes.
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/todos", get(list).post(create))
        .route("/api/todos/{id}", get(fetch))
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
async fn list(
    State(state): State<AppState>,
    PageQuery(page): PageQuery,
) -> Result<Json<Vec<TodoDto>>, ApiError> {
    let response = client(&state)
        .list_todos(ListTodosRequest {
            page: Some(toolbox_grpc::PageRequestProto::from(&page)),
        })
        .await
        .map_err(|s| from_backend(&s))?
        .into_inner();

    Ok(Json(
        response.items.into_iter().map(TodoDto::from).collect(),
    ))
}

/// `GET`: one todo.
///
/// # Arguments
///
/// * `state` - The gateway's state, for the backend channel.
/// * `id` - Which todo. A miss is the backend's `TODO_NOT_FOUND`, relayed with
///   its code intact.
async fn fetch(
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

/// `POST`: add a todo. Writing needs the admin role, and the type system checks
/// it.
///
/// # Arguments
///
/// * `_` - The extractor that enforces the role. An anonymous caller is a 401
///   and never reaches the backend.
/// * `state` - The gateway's state, for the backend channel.
/// * `body` - The new todo, rejected here if invalid so no hop is made.
async fn create(
    _: Authenticated<Admin>,
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
