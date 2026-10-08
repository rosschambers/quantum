//! Repository review (qv diff mode) service.
//!
//! Wraps the domain `RepositoryReview` port for the `file-viewer.changes` /
//! `file-viewer.stage` / `file-viewer.unstage` / `file-viewer.fingerprint`
//! IPC surface, and owns the staging safety boundary from the design doc:
//! `stage`/`unstage` only ever touch a path that appeared in the most
//! recently loaded STAGEABLE change set for that repository root. This
//! crate depends only on `quantum_domain`; it never touches infrastructure
//! directly.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use quantum_domain::{ChangeSet, DiffSpec, RepositoryReview, ReviewError};
use tokio::sync::Mutex;

/// Orchestrates repository-review operations over the domain port, adding
/// the staging safety boundary the git layer itself does not know about:
/// git only validates that a path does not escape the repository; this
/// service additionally refuses a path that was never shown to the user in
/// the most recently loaded (and stageable) change set.
pub struct ReviewService {
    review: Arc<dyn RepositoryReview>,
    /// Per-repository-root set of stageable paths from the most recent
    /// `changes()` call. A root is absent when it has never loaded a
    /// stageable change set, or when its most recently loaded change set
    /// was not stageable (any base other than `HEAD`, decision D2).
    stageable_paths: Mutex<HashMap<String, HashSet<String>>>,
}

impl ReviewService {
    pub fn new(review: Arc<dyn RepositoryReview>) -> Self {
        Self {
            review,
            stageable_paths: Mutex::new(HashMap::new()),
        }
    }

    /// Load a repository's changes, remembering its path set for the
    /// staging safety boundary when (and only when) it is stageable.
    pub async fn changes(&self, spec: &DiffSpec) -> Result<ChangeSet, ReviewError> {
        let change_set = self.review.changes(spec).await?;

        let mut stageable_paths = self.stageable_paths.lock().await;
        if change_set.stageable {
            let paths: HashSet<String> = change_set
                .files
                .iter()
                .map(|file| file.path.clone())
                .collect();
            stageable_paths.insert(change_set.repository_root.clone(), paths);
        } else {
            // A non-stageable change set clears the remembered set for that
            // root: stage/unstage must never fall back to a stale set from
            // an earlier, stageable load of the same repository.
            stageable_paths.remove(&change_set.repository_root);
        }

        Ok(change_set)
    }

    pub async fn stage(
        &self,
        repository_root: &str,
        path: &str,
        blob: Option<&str>,
        mode: &str,
    ) -> Result<(), ReviewError> {
        self.check_stageable(repository_root, path).await?;
        self.review.stage(repository_root, path, blob, mode).await
    }

    pub async fn unstage(&self, repository_root: &str, path: &str) -> Result<(), ReviewError> {
        self.check_stageable(repository_root, path).await?;
        self.review.unstage(repository_root, path).await
    }

    pub async fn fingerprint(&self, repository_root: &str) -> Result<String, ReviewError> {
        self.review.fingerprint(repository_root).await
    }

    /// The staging safety boundary: refuse any path that did not appear in
    /// the most recently loaded stageable change set for `repository_root`.
    async fn check_stageable(&self, repository_root: &str, path: &str) -> Result<(), ReviewError> {
        let stageable_paths = self.stageable_paths.lock().await;
        let allowed = stageable_paths
            .get(repository_root)
            .is_some_and(|paths| paths.contains(path));

        if allowed {
            Ok(())
        } else {
            Err(ReviewError::NotStageable(format!(
                "{path} was not in the most recently loaded stageable change set for {repository_root}"
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use quantum_domain::ChangedFile;
    use std::sync::Mutex as StdMutex;

    #[derive(Default)]
    struct FakeReview {
        changes_result: StdMutex<Option<ChangeSet>>,
        stage_calls: StdMutex<Vec<(String, String)>>,
    }

    #[async_trait]
    impl RepositoryReview for FakeReview {
        async fn changes(&self, _spec: &DiffSpec) -> Result<ChangeSet, ReviewError> {
            Ok(self
                .changes_result
                .lock()
                .expect("lock")
                .clone()
                .expect("changes_result must be set before calling changes()"))
        }

        async fn stage(
            &self,
            repository_root: &str,
            path: &str,
            _blob: Option<&str>,
            _mode: &str,
        ) -> Result<(), ReviewError> {
            self.stage_calls
                .lock()
                .expect("lock")
                .push((repository_root.to_string(), path.to_string()));
            Ok(())
        }

        async fn unstage(&self, _repository_root: &str, _path: &str) -> Result<(), ReviewError> {
            Ok(())
        }

        async fn fingerprint(&self, _repository_root: &str) -> Result<String, ReviewError> {
            Ok("abc123".to_string())
        }
    }

    fn stageable_change_set(root: &str, path: &str) -> ChangeSet {
        ChangeSet {
            repository_root: root.to_string(),
            base_label: "HEAD".to_string(),
            target_label: "working tree".to_string(),
            stageable: true,
            files: vec![ChangedFile {
                path: path.to_string(),
                old_path: None,
                language: None,
                base: None,
                index: None,
                target: None,
                untracked: false,
            }],
        }
    }

    fn diff_spec(repository: &str) -> DiffSpec {
        DiffSpec {
            repository: repository.to_string(),
            base: "HEAD".to_string(),
            target: None,
        }
    }

    #[tokio::test]
    async fn stage_succeeds_for_a_path_in_the_last_loaded_change_set() {
        let fake = Arc::new(FakeReview::default());
        *fake.changes_result.lock().expect("lock") = Some(stageable_change_set("/repo", "a.txt"));
        let service = ReviewService::new(fake.clone());

        service.changes(&diff_spec("/repo")).await.expect("changes");
        service
            .stage("/repo", "a.txt", Some("blob"), "100644")
            .await
            .expect("stage succeeds for a path in the loaded change set");

        assert_eq!(fake.stage_calls.lock().expect("lock").len(), 1);
    }

    #[tokio::test]
    async fn stage_rejects_a_path_outside_the_last_loaded_change_set() {
        let fake = Arc::new(FakeReview::default());
        *fake.changes_result.lock().expect("lock") = Some(stageable_change_set("/repo", "a.txt"));
        let service = ReviewService::new(fake);

        service.changes(&diff_spec("/repo")).await.expect("changes");

        let error = service
            .stage("/repo", "other.txt", Some("blob"), "100644")
            .await
            .expect_err("a path outside the loaded change set must be rejected");
        assert!(matches!(error, ReviewError::NotStageable(_)));
    }

    #[tokio::test]
    async fn stage_rejects_everything_before_any_changes_call() {
        let fake = Arc::new(FakeReview::default());
        let service = ReviewService::new(fake);

        let error = service
            .stage("/repo", "a.txt", Some("blob"), "100644")
            .await
            .expect_err("staging before any changes() call must be rejected");
        assert!(matches!(error, ReviewError::NotStageable(_)));
    }

    #[tokio::test]
    async fn unstage_rejects_a_path_outside_the_last_loaded_change_set() {
        let fake = Arc::new(FakeReview::default());
        *fake.changes_result.lock().expect("lock") = Some(stageable_change_set("/repo", "a.txt"));
        let service = ReviewService::new(fake);

        service.changes(&diff_spec("/repo")).await.expect("changes");

        let error = service
            .unstage("/repo", "other.txt")
            .await
            .expect_err("a path outside the loaded change set must be rejected");
        assert!(matches!(error, ReviewError::NotStageable(_)));
    }

    #[tokio::test]
    async fn a_non_stageable_change_set_clears_the_remembered_paths() {
        let fake = Arc::new(FakeReview::default());
        *fake.changes_result.lock().expect("lock") = Some(stageable_change_set("/repo", "a.txt"));
        let service = ReviewService::new(fake.clone());
        service.changes(&diff_spec("/repo")).await.expect("changes");

        // Load a non-stageable change set for the same root (any base
        // other than HEAD).
        let mut non_stageable = stageable_change_set("/repo", "a.txt");
        non_stageable.stageable = false;
        *fake.changes_result.lock().expect("lock") = Some(non_stageable);
        service.changes(&diff_spec("/repo")).await.expect("changes");

        let error = service
            .stage("/repo", "a.txt", Some("blob"), "100644")
            .await
            .expect_err("a non-stageable change set must clear the remembered paths");
        assert!(matches!(error, ReviewError::NotStageable(_)));
    }

    #[tokio::test]
    async fn fingerprint_delegates_to_the_port() {
        let fake = Arc::new(FakeReview::default());
        let service = ReviewService::new(fake);
        assert_eq!(
            service.fingerprint("/repo").await.expect("fingerprint"),
            "abc123"
        );
    }
}
