/*!
 * Thin async wrappers around the external command-line tools tharhosting
 * drives: `git` and `docker`. Shared command-running and logging helpers
 * live in [`process`].
 */

pub mod docker;
pub mod git;
pub mod process;
