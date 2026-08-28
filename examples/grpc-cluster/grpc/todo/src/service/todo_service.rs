//! `todo.v1.TodoService`, and what it can fail with.
//!
//! Note what is *not* here: no `EntityService`, no `DatabaseService`, no
//! `Repository`. A service is a struct with a `new` and an `into_server`.

use std::collections::BTreeMap;

use tonic::{Request, Response};
use toolbox_core::{ErrorKind, ServiceError};
use toolbox_db::{Db, DbError};
use toolbox_grpc::GrpcResult;

use crate::{
    Connection,
    model::Todo,
    proto,
    proto::{
        CompleteTodoRequest, CreateTodoRequest, DeleteTodoRequest, DeleteTodoResponse,
        GetTodoRequest, ListTodosRequest, ListTodosResponse, todo_service_server,
    },
};

/// The todo service.
#[derive(Clone)]
pub struct TodoService {
    db: Db<Connection>,
}

impl TodoService {
    /// Build the service over a pool.
    ///
    /// # Arguments
    ///
    /// * `db` - The pool it reads and writes through. Every call goes via
    ///   `Db::run` or `Db::transaction`, so no diesel call blocks the runtime.
    #[must_use]
    pub fn new(db: Db<Connection>) -> Self {
        Self { db }
    }

    /// Wrap it as a mountable tonic service.
    #[must_use]
    pub fn into_server(self) -> todo_service_server::TodoServiceServer<Self> {
        todo_service_server::TodoServiceServer::new(self)
    }
}

#[tonic::async_trait]
impl todo_service_server::TodoService for TodoService {
    async fn get_todo(&self, request: Request<GetTodoRequest>) -> GrpcResult<proto::Todo> {
        let id = request.into_inner().id;
        let todo = self
            .db
            .run_named("get_todo", move |c: &mut Connection| {
                Todo::find_by_id(c, &id)?.ok_or(TodoServiceError::NotFound(id))
            })
            .await?;
        Ok(Response::new(todo.into()))
    }

    async fn list_todos(
        &self,
        request: Request<ListTodosRequest>,
    ) -> GrpcResult<ListTodosResponse> {
        let request = request.into_inner();
        let page_request = request
            .page
            .unwrap_or_default()
            .to_domain()
            .map_err(|e| tonic::Status::invalid_argument(e.to_string()))?;
        let needle = request.title_contains;

        let page = self
            .db
            .run_named("list_todos", move |c: &mut Connection| {
                if needle.is_empty() {
                    Todo::page(c, &page_request).map_err(TodoServiceError::from)
                } else {
                    Todo::search(c, &needle, &page_request).map_err(TodoServiceError::from)
                }
            })
            .await?;

        // Page::try_map plus split is the whole conversion block that used to
        // be written out per handler.
        let (items, page) = toolbox_grpc::split(page.map(proto::Todo::from));
        Ok(Response::new(ListTodosResponse {
            items,
            page: Some(page),
        }))
    }

    async fn create_todo(&self, request: Request<CreateTodoRequest>) -> GrpcResult<proto::Todo> {
        let title = request.into_inner().title;
        if title.trim().is_empty() {
            return Err(TodoServiceError::EmptyTitle.into());
        }

        let todo = self
            .db
            .run_named("create_todo", move |c: &mut Connection| {
                Todo::new(title).save(c).map_err(TodoServiceError::from)
            })
            .await?;
        Ok(Response::new(todo.into()))
    }

    async fn complete_todo(
        &self,
        request: Request<CompleteTodoRequest>,
    ) -> GrpcResult<proto::Todo> {
        let CompleteTodoRequest { id, version } = request.into_inner();

        let todo = self
            .db
            .transaction(move |c: &mut Connection| {
                let mut todo = Todo::find_by_id(c, &id)?.ok_or(TodoServiceError::NotFound(id))?;
                todo.done = true;
                todo.version = version;
                todo.save(c).map_err(|e| match e {
                    // The derive returns Conflict when the version check
                    // matched no rows; the service says which todo.
                    DbError::Conflict => TodoServiceError::Conflict(id),
                    other => TodoServiceError::Db(other),
                })
            })
            .await?;
        Ok(Response::new(todo.into()))
    }

    async fn delete_todo(
        &self,
        request: Request<DeleteTodoRequest>,
    ) -> GrpcResult<DeleteTodoResponse> {
        let id = request.into_inner().id;
        let deleted = self
            .db
            .run_named("delete_todo", move |c: &mut Connection| {
                Todo::delete_by_id(c, &id).map_err(TodoServiceError::from)
            })
            .await?;

        Ok(Response::new(DeleteTodoResponse {
            deleted: i32::try_from(deleted).unwrap_or(i32::MAX),
        }))
    }
}

/// What this service can fail with.
///
/// Beside the service rather than in an `error.rs` of its own: the variants are
/// its return type, and a second service in this domain fails differently.
#[derive(Debug, thiserror::Error)]
pub enum TodoServiceError {
    /// No todo with that id.
    #[error("todo {0} not found")]
    NotFound(i32),
    /// Someone else changed it first.
    #[error("todo {0} was changed by someone else")]
    Conflict(i32),
    /// The title was empty.
    #[error("a todo needs a title")]
    EmptyTitle,
    /// Anything the database refused.
    #[error(transparent)]
    Db(#[from] DbError),
    /// A rollback, needed for `Db::transaction`'s bound.
    #[error(transparent)]
    Query(#[from] diesel::result::Error),
}

impl ServiceError for TodoServiceError {
    fn code(&self) -> &'static str {
        match self {
            Self::NotFound(_) => "TODO_NOT_FOUND",
            Self::Conflict(_) => "TODO_CONFLICT",
            Self::EmptyTitle => "TODO_EMPTY_TITLE",
            Self::Db(e) => e.code(),
            Self::Query(_) => "TODO_QUERY_FAILED",
        }
    }

    fn domain(&self) -> &'static str {
        "todo"
    }

    fn kind(&self) -> ErrorKind {
        match self {
            Self::NotFound(_) => ErrorKind::NotFound,
            Self::Conflict(_) => ErrorKind::Conflict,
            Self::EmptyTitle => ErrorKind::InvalidArgument,
            Self::Db(e) => e.kind(),
            Self::Query(_) => ErrorKind::Internal,
        }
    }

    fn metadata(&self) -> BTreeMap<String, String> {
        match self {
            Self::NotFound(id) | Self::Conflict(id) => {
                BTreeMap::from([("id".to_owned(), id.to_string())])
            }
            Self::Db(e) => e.metadata(),
            _ => BTreeMap::new(),
        }
    }
}

/// The one line every consumer writes, and the deliberate reason there is no
/// blanket impl in `toolbox-grpc`: this error is ours, so this is legal.
impl From<TodoServiceError> for tonic::Status {
    fn from(e: TodoServiceError) -> Self {
        toolbox_grpc::to_status(e)
    }
}
