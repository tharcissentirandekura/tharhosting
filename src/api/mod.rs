/*!
 * The REST API.
 *
 * - [`routes`]: the router and the app/deployment handlers
 * - [`webhook`]: the GitHub push webhook
 * - [`auth`]: bearer-token middleware
 * - [`error`]: the shared error type and how it becomes a response
 * - [`payloads`] and [`github`]: request body shapes
 * - [`validation`]: input checks
 */

pub mod auth;
pub mod error;
pub mod github;
pub mod payloads;
pub mod routes;
pub mod validation;
pub mod webhook;
pub mod logging;
pub use routes::router;
