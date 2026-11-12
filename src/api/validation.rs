/*!
 * Validation of API input: names, URLs, branches, env vars and ports, plus
 * normalization of git URLs so equivalent forms compare equal.
 */

use std::collections::BTreeMap;

use super::error::AppError;

/**
 * Checks that `name` is a valid app name.
 *
 * Rules (a subset of DNS-label syntax):
 * - 1 to 40 characters
 * - only lowercase `a-z`, digits `0-9` and `-`
 * - must not start or end with `-`
 *
 * # Errors
 *
 * Returns [`AppError::BadRequest`] describing the rules if any is violated.
 */
pub fn validate_name(name: &str) -> Result<(), AppError> {
    const MAX_NAME_LEN: usize = 40;
    let is_allowed = |b: u8| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'-');

    let valid = (1..=MAX_NAME_LEN).contains(&name.len())
        && name.bytes().all(is_allowed)
        && !name.starts_with('-')
        && !name.ends_with('-');

    if valid {
        Ok(())
    } else {
        Err(AppError::BadRequest(format!(
            "name must be 1-{MAX_NAME_LEN} chars of a-z, 0-9 and '-', \
             not starting or ending with '-'"
        )))
    }
}

/**
 * Checks that `url` is a plausible git remote before it is stored or cloned.
 *
 * Accepted forms:
 * - `https://host/username/repo.git` and `http://...`
 * - `ssh://git@host/username/repo.git`
 * - `git@host:username/repo.git` (scp-style)
 * - `file:///path/to/repo`
 *
 * This only checks the *shape* of the URL. It does not resolve the host or
 * confirm the repository exists.
 *
 * # Errors
 *
 * Returns [`AppError::BadRequest`] if the URL does not start with one of
 * the accepted forms above, or contains whitespace or control characters.
 */
pub fn validate_repo_url(url: &str) -> Result<(), AppError> {
    /**
     * URL prefixes that `git clone` is allowed to fetch from.
     *
     * Every entry starts with a letter, so a URL can never begin with `-` and be
     * parsed by git as a command-line option (e.g. `--upload-pack=...`).
     */
    const ALLOWED_URL_PREFIXES: [&str; 5] = ["https://", "http://", "ssh://", "git@", "file://"];

    let has_allowed_prefix = ALLOWED_URL_PREFIXES.iter().any(|p| url.starts_with(p));
    let has_bad_chars = url.chars().any(|c| c.is_whitespace() || c.is_control());

    if has_allowed_prefix && !has_bad_chars {
        Ok(())
    } else {
        Err(AppError::BadRequest(
            "Repository URL must be an https://, http://, ssh://, git@ or file:// git URL".into(),
        ))
    }
}

/**
 * Checks that `branch` is a safe, well-formed git branch name.
 *
 * Rules:
 * - not empty
 * - only ASCII letters, digits and `.` `_` `/` `-`
 * - must not start with `-`, so git can't parse it as a command-line option
 * - must not contain `..`, which git reserves for revision ranges
 *   (and which blocks path-traversal-style names)
 *
 * This is stricter than git itself (no unicode branch names), which is
 * intentional: the name is passed to `git` and used on disk.
 *
 * # Errors
 *
 * Returns [`AppError::BadRequest`] if any rule is violated.
 */
pub fn validate_branch(branch: &str) -> Result<(), AppError> {
    const BRANCH_EXTRA_CHARS: &[u8] = b"._/-";

    let is_allowed = |b: u8| b.is_ascii_alphanumeric() || BRANCH_EXTRA_CHARS.contains(&b);

    let valid = !branch.is_empty()
        && !branch.starts_with('-')
        && !branch.contains("..")
        && branch.bytes().all(is_allowed);

    if valid {
        Ok(())
    } else {
        Err(AppError::BadRequest("invalid branch name".into()))
    }
}

/**
 * Checks that every key in `env` is a valid environment variable name.
 *
 * Rules (POSIX-style names):
 * - not empty
 * - only ASCII letters, digits and `_`
 * - must not start with a digit
 *
 * Only the names are checked. Values can be any string.
 *
 * # Errors
 *
 * Returns [`AppError::BadRequest`] naming the first invalid key.
 */
pub fn validate_env(env: &BTreeMap<String, String>) -> Result<(), AppError> {
    let is_valid_name = |name: &str| {
        !name.is_empty()
            && !name.starts_with(|c: char| c.is_ascii_digit())
            && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
    };

    match env.keys().find(|k| !is_valid_name(k)) {
        Some(bad) => Err(AppError::BadRequest(format!(
            "invalid env var name: {bad:?}"
        ))),
        None => Ok(()),
    }
}

/**
 * Checks that `path` is a safe Dockerfile location inside the repository.
 *
 * Rules:
 * - 1 to 100 characters: ASCII letters, digits and `.` `_` `-` `/`
 * - relative to the repository root, so it must not start with `/`
 * - no empty, `.` or `..` path segments, so it can't climb out of the repo
 *
 * This only checks the text. At deploy time the file must also exist and still
 * resolve inside the checkout, which catches symlinks that point elsewhere.
 *
 * # Errors
 *
 * Returns [`AppError::BadRequest`] describing the rules if any is violated.
 */
pub fn validate_dockerfile(path: &str) -> Result<(), AppError> {
    const MAX_LEN: usize = 100;
    const EXTRA_CHARS: &[u8] = b"._-/";

    let is_allowed = |b: u8| b.is_ascii_alphanumeric() || EXTRA_CHARS.contains(&b);

    let valid = (1..=MAX_LEN).contains(&path.len())
        && path.bytes().all(is_allowed)
        && !path.starts_with('/')
        && path.split('/').all(|part| !matches!(part, "" | "." | ".."));

    if valid {
        Ok(())
    } else {
        Err(AppError::BadRequest(format!(
            "dockerfile must be a relative path of 1-{MAX_LEN} chars (letters, digits, \
             '.', '_', '-', '/') with no '..' or empty parts"
        )))
    }
}

/**
 * Checks that `port` is a usable container port.
 *
 * `u16` already limits the value to 0-65535, so the only invalid value left is `0`.
 *
 * # Errors
 *
 * Returns [`AppError::BadRequest`] if `port` is `0`.
 */
pub fn validate_port(port: u16) -> Result<(), AppError> {
    if port == 0 {
        return Err(AppError::BadRequest("container_port must be 1-65535".into()));
    }
    Ok(())
}

/**
 * Reduces a git URL to `host/username/repo` so that the https, ssh, scp-style
 * and `.git` forms of the same repository compare equal.
 *
 * Steps, in order:
 * 1. trim and lowercase
 * 2. drop the scheme (`https://`, `ssh://`, ...)
 * 3. drop a `user@` prefix (`git@github.com:...`)
 * 4. turn the scp-style `host:username/repo` into `host/username/repo`
 * 5. drop trailing `/` and `.git`
 *
 * # Examples
 *
 * `https://GitHub.com/me/blog.git`, `git@github.com:me/blog` and
 * `ssh://git@github.com/me/blog/` all become `github.com/me/blog`.
 */
pub fn normalize_repo_url(url: &str) -> String {
    const SCHEMES: [&str; 4] = ["https://", "http://", "ssh://", "git://"];

    let url = url.trim().to_ascii_lowercase();

    let without_scheme = SCHEMES
        .iter()
        .find_map(|scheme| url.strip_prefix(scheme))
        .unwrap_or(url.as_str());

    // `user@host` is dropped, but an `@` inside the path (after a '/') is kept.
    let without_user = match without_scheme.split_once('@') {
        Some((user, remaining_part)) if !user.contains('/') => remaining_part,
        _ => without_scheme,
    };

    let path = without_user.replacen(':', "/", 1);
    let path = path.trim_end_matches('/');

    path.strip_suffix(".git").unwrap_or(path).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repo_urls_normalize_to_the_same_key() {
        let want = "github.com/thar/site";
        for u in [
            "https://github.com/Thar/site",
            "https://github.com/thar/site.git",
            "https://github.com/thar/site/",
            "git@github.com:thar/site.git",
            "ssh://git@github.com/thar/site.git",
        ] {
            assert_eq!(normalize_repo_url(u), want, "{u}");
        }
    }

    #[test]
    fn names_are_validated() {
        assert!(validate_name("my-app2").is_ok());
        for bad in ["", "-a", "a-", "My", "a_b", &"x".repeat(41)] {
            assert!(validate_name(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn branches_and_urls_are_validated() {
        assert!(validate_branch("feature/x-1.2").is_ok());
        assert!(validate_branch("--upload-pack=x").is_err());
        assert!(validate_repo_url("https://github.com/a/b").is_ok());
        assert!(validate_repo_url("--config=x").is_err());
        assert!(validate_repo_url("/etc").is_err());
    }

    #[test]
    fn dockerfile_paths_are_validated() {
        for ok in [
            "Dockerfile",
            "Dockerfile.prod",
            "app.Dockerfile",
            "docker/Dockerfile",
            "deploy/api/Dockerfile.prod",
        ] {
            assert!(validate_dockerfile(ok).is_ok(), "{ok}");
        }
        for bad in [
            "",
            "/etc/passwd",
            "../Dockerfile",
            "a/../../b",
            "a//b",
            "./Dockerfile",
            "docker/",
            "Dockerfile name",
            &"x".repeat(101),
        ] {
            assert!(validate_dockerfile(bad).is_err(), "{bad}");
        }
    }
}
