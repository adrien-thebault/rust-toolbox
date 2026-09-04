//! Shared fixture for the `openapi` module tests.

use utoipa::OpenApi;

mod augment;
mod router;
mod serialize;

#[derive(utoipa::ToSchema, serde::Serialize)]
struct Todo {
    id: i32,
    title: String,
}

#[utoipa::path(get, path = "/api/todos/{id}", responses((status = 200, body = Todo)))]
#[allow(dead_code)]
fn get_todo() {}

#[utoipa::path(post, path = "/api/todos", responses((status = 201, body = Todo)))]
#[allow(dead_code)]
fn create_todo() {}

#[derive(OpenApi)]
#[openapi(paths(get_todo, create_todo), components(schemas(Todo)))]
struct Api;
