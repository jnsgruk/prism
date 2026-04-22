use k8s_openapi::api::batch::v1::{Job, JobSpec};
use k8s_openapi::api::core::v1::{
    Container, EnvVar, EnvVarSource, PersistentVolumeClaimVolumeSource, Pod, PodSecurityContext,
    PodSpec, PodTemplateSpec, ResourceRequirements, SecretKeySelector, Volume, VolumeMount,
};
use k8s_openapi::apimachinery::pkg::api::resource::Quantity;
use kube::api::{DeleteParams, ListParams, ObjectMeta, PostParams};
use kube::{Api, Client};
use tonic::Status;
use tracing::info;

/// Status of a backup job.
pub enum BackupJobStatus {
    /// The job is still running.
    Running,
    /// The job completed successfully.
    Succeeded,
    /// The job failed with the given error message.
    Failed(String),
}

/// Trait abstracting backup generation so production uses K8s Jobs and tests
/// can run `pg_dump` directly without a cluster.
#[async_trait::async_trait]
pub trait BackupGenerator: Send + Sync {
    /// Start a new backup job. Returns once the job has been created.
    async fn start_backup(&self, backup_id: &str, exclude_workspaces: bool) -> Result<(), Status>;

    /// Start a restore job. The archive must already exist on the backups PVC
    /// at `<backups_path>/<backup_id>.ps-backup`.
    async fn start_restore(&self, backup_id: &str) -> Result<(), Status>;

    /// Cancel any active backup job.
    async fn cancel_backup(&self) -> Result<bool, Status>;

    /// Poll the status of a job (backup or restore) by its ID.
    async fn poll_status(&self, backup_id: &str) -> Result<BackupJobStatus, Status>;

    /// Check if a backup is already in progress.
    async fn is_backup_active(&self) -> Result<bool, Status>;

    /// Force-cancel any active backup jobs.
    async fn force_cancel(&self) -> Result<(), Status>;
}

/// Production implementation that manages K8s Jobs.
pub struct KubeBackupGenerator {
    kube_client: Client,
    namespace: String,
    backup_image: String,
}

impl KubeBackupGenerator {
    pub fn new(kube_client: Client, namespace: String, backup_image: String) -> Self {
        Self {
            kube_client,
            namespace,
            backup_image,
        }
    }

    fn job_name(backup_id: &str) -> String {
        let name = format!("ps-backup-{backup_id}");
        // K8s names are limited to 63 chars
        if name.len() > 63 {
            name[..63].to_owned()
        } else {
            name
        }
    }

    fn jobs_api(&self) -> Api<Job> {
        Api::namespaced(self.kube_client.clone(), &self.namespace)
    }

    fn build_backup_job(&self, backup_id: &str, exclude_workspaces: bool) -> Job {
        let mut extra_env = vec![EnvVar {
            name: "EXCLUDE_WORKSPACES".into(),
            value: Some(exclude_workspaces.to_string()),
            ..Default::default()
        }];
        // Backup mode: workspaces volume is read-only
        self.build_job(backup_id, "backup", &mut extra_env, true)
    }

    fn build_restore_job(&self, backup_id: &str) -> Job {
        // Restore mode: workspaces volume is read-write (files are extracted)
        self.build_job(backup_id, "restore", &mut vec![], false)
    }

    fn build_job(
        &self,
        backup_id: &str,
        mode: &str,
        extra_env: &mut Vec<EnvVar>,
        workspaces_read_only: bool,
    ) -> Job {
        let job_name = Self::job_name(backup_id);

        let mut env = vec![
            EnvVar {
                name: "DATABASE_URL".into(),
                value_from: Some(EnvVarSource {
                    secret_key_ref: Some(SecretKeySelector {
                        name: "prism-secrets".into(),
                        key: "DATABASE_URL".into(),
                        ..Default::default()
                    }),
                    ..Default::default()
                }),
                ..Default::default()
            },
            EnvVar {
                name: "PS_SECRET_KEY".into(),
                value_from: Some(EnvVarSource {
                    secret_key_ref: Some(SecretKeySelector {
                        name: "prism-secrets".into(),
                        key: "PS_SECRET_KEY".into(),
                        ..Default::default()
                    }),
                    ..Default::default()
                }),
                ..Default::default()
            },
            EnvVar {
                name: "MODE".into(),
                value: Some(mode.to_owned()),
                ..Default::default()
            },
            EnvVar {
                name: "BACKUP_ID".into(),
                value: Some(backup_id.to_owned()),
                ..Default::default()
            },
            EnvVar {
                name: "WORKSPACES_PATH".into(),
                value: Some("/workspaces".into()),
                ..Default::default()
            },
            EnvVar {
                name: "BACKUPS_PATH".into(),
                value: Some("/backups".into()),
                ..Default::default()
            },
        ];
        env.append(extra_env);

        Job {
            metadata: ObjectMeta {
                name: Some(job_name),
                namespace: Some(self.namespace.clone()),
                labels: Some(
                    [
                        ("app".to_owned(), "ps-backup".to_owned()),
                        ("backup-id".to_owned(), backup_id.to_owned()),
                    ]
                    .into(),
                ),
                ..Default::default()
            },
            spec: Some(JobSpec {
                backoff_limit: Some(1),
                active_deadline_seconds: Some(3600),
                ttl_seconds_after_finished: Some(300),
                template: PodTemplateSpec {
                    metadata: Some(ObjectMeta {
                        labels: Some([("app".to_owned(), "ps-backup".to_owned())].into()),
                        ..Default::default()
                    }),
                    spec: Some(PodSpec {
                        restart_policy: Some("Never".into()),
                        // fsGroup ensures mounted PVCs are group-writable
                        // regardless of which UID owns the underlying storage.
                        security_context: Some(PodSecurityContext {
                            fs_group: Some(65534),
                            ..Default::default()
                        }),
                        containers: vec![Container {
                            name: "backup".into(),
                            image: Some(self.backup_image.clone()),
                            env: Some(env),
                            volume_mounts: Some(vec![
                                VolumeMount {
                                    name: "workspaces".into(),
                                    mount_path: "/workspaces".into(),
                                    read_only: Some(workspaces_read_only),
                                    ..Default::default()
                                },
                                VolumeMount {
                                    name: "backups".into(),
                                    mount_path: "/backups".into(),
                                    ..Default::default()
                                },
                            ]),
                            resources: Some(ResourceRequirements {
                                requests: Some(
                                    [
                                        ("memory".into(), Quantity("256Mi".into())),
                                        ("cpu".into(), Quantity("200m".into())),
                                    ]
                                    .into(),
                                ),
                                limits: Some([("memory".into(), Quantity("512Mi".into()))].into()),
                                ..Default::default()
                            }),
                            ..Default::default()
                        }],
                        volumes: Some(vec![
                            Volume {
                                name: "workspaces".into(),
                                persistent_volume_claim: Some(PersistentVolumeClaimVolumeSource {
                                    claim_name: "prism-workspaces".into(),
                                    read_only: Some(workspaces_read_only),
                                }),
                                ..Default::default()
                            },
                            Volume {
                                name: "backups".into(),
                                persistent_volume_claim: Some(PersistentVolumeClaimVolumeSource {
                                    claim_name: "prism-backups".into(),
                                    read_only: Some(false),
                                }),
                                ..Default::default()
                            },
                        ]),
                        ..Default::default()
                    }),
                },
                ..Default::default()
            }),
            ..Default::default()
        }
    }
}

/// Check if a Job is still active (neither succeeded nor failed).
fn is_job_active(job: &Job) -> bool {
    job.status
        .as_ref()
        .is_none_or(|s| s.succeeded.unwrap_or(0) == 0 && s.failed.unwrap_or(0) == 0)
}

/// Try to extract an error message from a failed Job's pod.
async fn get_job_failure_message(kube_client: &Client, namespace: &str, backup_id: &str) -> String {
    let pods: Api<Pod> = Api::namespaced(kube_client.clone(), namespace);
    let lp = ListParams::default().labels(&format!("backup-id={backup_id}"));
    match pods.list(&lp).await {
        Ok(pod_list) => {
            for pod in &pod_list.items {
                if let Some(status) = &pod.status
                    && let Some(container_statuses) = &status.container_statuses
                {
                    for cs in container_statuses {
                        if let Some(terminated) =
                            cs.state.as_ref().and_then(|s| s.terminated.as_ref())
                        {
                            if let Some(ref msg) = terminated.message {
                                return msg.clone();
                            }
                            return format!("container exited with code {}", terminated.exit_code);
                        }
                    }
                }
            }
            "backup job failed (no details available)".into()
        }
        Err(_) => "backup job failed (could not read pod status)".into(),
    }
}

#[async_trait::async_trait]
impl BackupGenerator for KubeBackupGenerator {
    async fn start_backup(&self, backup_id: &str, exclude_workspaces: bool) -> Result<(), Status> {
        let jobs = self.jobs_api();
        let job = self.build_backup_job(backup_id, exclude_workspaces);
        jobs.create(&PostParams::default(), &job)
            .await
            .map_err(|e| Status::internal(format!("failed to create backup job: {e}")))?;
        info!(backup_id = %backup_id, "created K8s backup job");
        Ok(())
    }

    async fn start_restore(&self, backup_id: &str) -> Result<(), Status> {
        let jobs = self.jobs_api();
        let job = self.build_restore_job(backup_id);
        jobs.create(&PostParams::default(), &job)
            .await
            .map_err(|e| Status::internal(format!("failed to create restore job: {e}")))?;
        info!(backup_id = %backup_id, "created K8s restore job");
        Ok(())
    }

    async fn cancel_backup(&self) -> Result<bool, Status> {
        let jobs = self.jobs_api();
        let lp = ListParams::default().labels("app=ps-backup");
        let list = jobs
            .list(&lp)
            .await
            .map_err(|e| Status::internal(format!("failed to list backup jobs: {e}")))?;

        let mut cancelled = false;
        for job in &list.items {
            if is_job_active(job)
                && let Some(name) = &job.metadata.name
            {
                let dp = DeleteParams::background();
                jobs.delete(name, &dp)
                    .await
                    .map_err(|e| Status::internal(format!("failed to delete backup job: {e}")))?;
                info!(job = %name, "backup job cancelled");
                cancelled = true;
            }
        }

        Ok(cancelled)
    }

    async fn poll_status(&self, backup_id: &str) -> Result<BackupJobStatus, Status> {
        let jobs = self.jobs_api();
        let job_name = Self::job_name(backup_id);
        let job = jobs
            .get(&job_name)
            .await
            .map_err(|e| Status::internal(format!("failed to get backup job: {e}")))?;

        if let Some(ref status) = job.status {
            if status.succeeded.unwrap_or(0) > 0 {
                return Ok(BackupJobStatus::Succeeded);
            }
            if status.failed.unwrap_or(0) > 0 {
                let msg =
                    get_job_failure_message(&self.kube_client, &self.namespace, backup_id).await;
                return Ok(BackupJobStatus::Failed(msg));
            }
        }

        Ok(BackupJobStatus::Running)
    }

    async fn is_backup_active(&self) -> Result<bool, Status> {
        let jobs = self.jobs_api();
        let lp = ListParams::default().labels("app=ps-backup");
        let list = jobs
            .list(&lp)
            .await
            .map_err(|e| Status::internal(format!("failed to list backup jobs: {e}")))?;

        Ok(list.items.iter().any(is_job_active))
    }

    async fn force_cancel(&self) -> Result<(), Status> {
        let jobs = self.jobs_api();
        let lp = ListParams::default().labels("app=ps-backup");
        let list = jobs
            .list(&lp)
            .await
            .map_err(|e| Status::internal(format!("failed to list backup jobs: {e}")))?;

        for job in &list.items {
            if let Some(name) = &job.metadata.name {
                let dp = DeleteParams::background();
                let _ = jobs.delete(name, &dp).await;
            }
        }

        Ok(())
    }
}
