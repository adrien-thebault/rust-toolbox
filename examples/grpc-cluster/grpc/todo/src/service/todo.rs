//! `todo.v1.TodoService`, and what it can fail with.
//!
//! Note what is *not* here: no `EntityService`, no `DatabaseService`, no
//! `Repository`. A service is a struct with a `new` and an `into_server`.

use std::{pin::Pin, sync::Arc};

use chrono::NaiveDateTime;
use cloudevents::AttributesReader as _;
use tokio_stream::{Stream, StreamExt as _};
use tonic::{Request, Response, Status};
use toolbox_cluster::{EventBus, Topic, event, payload};
use toolbox_core::ServiceError;
use toolbox_db::{Db, DbError};
use toolbox_grpc::{GrpcResult, server::identity};
use tracing::{info, trace, warn};

use crate::{
    Connection, EVENT_SOURCE, TODOS_TOPIC,
    auth::Admin,
    model::Todo,
    proto,
    proto::{
        CompleteTodoRequest, CreateTodoRequest, DeleteTodoRequest, DeleteTodoResponse,
        GetTodoRequest, ListTodosRequest, ListTodosResponse, TodoEvent, WatchTodosRequest,
        todo_service_server,
    },
};

/// The todo service.
#[derive(Clone)]
pub struct TodoService {
    /// The pool every handler runs its queries on.
    db: Db<Connection>,
    /// Where every mutation, including the scheduled purge, is published.
    ///
    /// In-process only: `WatchTodos` subscribes to it and the gateway holds
    /// that stream open. A second backend replica would need a shared adapter
    /// (Redis, Kafka) here so one replica's purge is seen by a gateway talking
    /// to another - the exact seam the trait exists for.
    events: Arc<dyn EventBus>,
}

impl TodoService {
    /// Build the service over a pool and an event bus.
    ///
    /// # Arguments
    ///
    /// * `db` - The pool it reads and writes through. Every call goes via
    ///   `Db::run` or `Db::transaction`, so no diesel call blocks the runtime.
    /// * `events` - Where each mutation publishes a `todo.*` event, and what
    ///   `WatchTodos` streams to the gateway.
    #[must_use]
    pub fn new(db: Db<Connection>, events: Arc<dyn EventBus>) -> Self {
        Self { db, events }
    }

    /// Wrap it as a mountable tonic service.
    #[must_use]
    pub fn into_server(self) -> todo_service_server::TodoServiceServer<Self> {
        todo_service_server::TodoServiceServer::new(self)
    }

    /// Publish one `todo.*` event, best-effort.
    ///
    /// A mutation that could not be announced still happened, so a publish
    /// failure is logged and swallowed rather than failing the call that
    /// already succeeded.
    ///
    /// # Arguments
    ///
    /// * `ty` - The CloudEvents type, e.g. `"todo.created"`.
    /// * `id` - Which todo it was.
    async fn emit(&self, ty: &str, id: i32) {
        match event(ty, EVENT_SOURCE, &serde_json::json!({ "id": id })) {
            Ok(ev) => match self.events.publish(&Topic::from(TODOS_TOPIC), ev).await {
                Ok(()) => info!(ty, id, "published a todo event"),
                Err(e) => warn!(error = %e, ty, id, "could not publish a todo event"),
            },
            Err(e) => warn!(error = %e, ty, id, "could not build a todo event"),
        }
    }

    /// Soft-delete every completed todo last touched before `cutoff`, emitting
    /// a `todo.deleted` for each row. Driven by the scheduler in `main.rs`.
    ///
    /// The sweep itself is [`Todo::purge_completed_before`]; it returns the
    /// purged ids and this announces them, the half only the service can do.
    ///
    /// # Arguments
    ///
    /// * `cutoff` - Todos completed and not touched since before this are
    ///   purged.
    ///
    /// # Errors
    /// [`TodoServiceError::Db`] when the sweep query fails.
    pub async fn purge_completed(&self, cutoff: NaiveDateTime) -> Result<u64, TodoServiceError> {
        let ids = self
            .db
            .run_named("purge_completed", move |c: &mut Connection| {
                Todo::purge_completed_before(c, cutoff).map_err(TodoServiceError::from)
            })
            .await?;

        info!(count = ids.len(), "purged completed todos");

        for id in &ids {
            self.emit("todo.deleted", *id).await;
        }

        Ok(ids.len() as u64)
    }
}

#[tonic::async_trait]
impl todo_service_server::TodoService for TodoService {
    /// Every `todo.*` event on the bus, as a `TodoEvent`.
    type WatchTodosStream = Pin<Box<dyn Stream<Item = Result<TodoEvent, Status>> + Send>>;

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
            .map_err(|e| Status::invalid_argument(e.to_string()))?;
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
        self.emit("todo.created", todo.id).await;
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
        self.emit("todo.completed", todo.id).await;
        Ok(Response::new(todo.into()))
    }

    async fn delete_todo(
        &self,
        request: Request<DeleteTodoRequest>,
    ) -> GrpcResult<DeleteTodoResponse> {
        // Checked here too, not just at the gateway: `identity_layer` resolves
        // whoever `shared_secret_layer` let through, but only this handler
        // knows that *delete* is the operation that needs the admin role.
        if !identity::require(&request)?.has::<Admin>() {
            return Err(TodoServiceError::Forbidden.into());
        }
        let id = request.into_inner().id;
        let deleted = self
            .db
            .run_named("delete_todo", move |c: &mut Connection| {
                Todo::delete_by_id(c, &id).map_err(TodoServiceError::from)
            })
            .await?;
        if deleted > 0 {
            self.emit("todo.deleted", id).await;
        }

        Ok(Response::new(DeleteTodoResponse {
            deleted: i32::try_from(deleted).unwrap_or(i32::MAX),
        }))
    }

    async fn watch_todos(
        &self,
        _request: Request<WatchTodosRequest>,
    ) -> GrpcResult<Self::WatchTodosStream> {
        let events = self
            .events
            .subscribe(&Topic::from(TODOS_TOPIC))
            .await
            .map_err(|e| {
                warn!(error = %e, "could not subscribe to the todo event bus");
                Status::unavailable(e.to_string())
            })?;
        info!("a client attached to the todo event stream");

        let stream = events.map(|ev| {
            let id = payload::<serde_json::Value>(&ev)
                .ok()
                .and_then(|v| v.get("id").and_then(serde_json::Value::as_i64))
                .and_then(|n| i32::try_from(n).ok())
                .unwrap_or_default();
            let ty = ev.ty().to_owned();
            trace!(r#type = %ty, id, "sending a todo event down the stream");
            Ok(TodoEvent { r#type: ty, id })
        });
        Ok(Response::new(Box::pin(stream)))
    }
}

/// What this service can fail with.
///
/// Beside the service rather than in an `error.rs` of its own: the variants are
/// its return type, and a second service in this domain fails differently.
///
/// `#[derive(ServiceError)]` writes `code`/`domain`/`kind`/`metadata` from the
/// per-variant `#[service_error(..)]`, and `status` on the container adds the
/// `impl From<Self> for tonic::Status` every consumer would otherwise write by
/// hand. `transparent` delegates to the inner error's own impl.
#[derive(Debug, thiserror::Error, ServiceError)]
#[service_error(domain = "todo", status)]
pub enum TodoServiceError {
    /// No todo with that id.
    #[error("todo {0} not found")]
    #[service_error(code = "TODO_NOT_FOUND", kind = NotFound, meta(id = 0))]
    NotFound(i32),
    /// Someone else changed it first.
    #[error("todo {0} was changed by someone else")]
    #[service_error(code = "TODO_CONFLICT", kind = Conflict, meta(id = 0))]
    Conflict(i32),
    /// The title was empty.
    #[error("a todo needs a title")]
    #[service_error(code = "TODO_EMPTY_TITLE", kind = InvalidArgument)]
    EmptyTitle,
    /// The caller is not the admin.
    #[error("the admin role is required")]
    #[service_error(code = "TODO_FORBIDDEN", kind = PermissionDenied)]
    Forbidden,
    /// Anything the database refused.
    #[error(transparent)]
    #[service_error(transparent)]
    Db(#[from] DbError),
    /// A rollback, needed for `Db::transaction`'s bound.
    #[error(transparent)]
    #[service_error(code = "TODO_QUERY_FAILED", kind = Internal)]
    Query(#[from] diesel::result::Error),
}
