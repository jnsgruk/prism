use crate::Error;
use time::OffsetDateTime;
use uuid::Uuid;

use super::ReasoningRepo;

// ---------------------------------------------------------------------------
// Conversation types
// ---------------------------------------------------------------------------

/// A conversation record.
pub struct Conversation {
    pub id: Uuid,
    pub user_id: Uuid,
    pub title: Option<String>,
    pub status: String,
    pub model_name: String,
    pub container_pod_name: Option<String>,
    pub container_status: String,
    pub opencode_session_id: Option<String>,
    pub total_tool_calls: i32,
    pub total_prompt_tokens: i32,
    pub total_completion_tokens: i32,
    pub total_estimated_cost_usd: f32,
    pub query_status: String,
    pub created_at: OffsetDateTime,
    pub last_activity_at: OffsetDateTime,
}

/// A row from `reasoning.conversation_events`.
pub struct ConversationEvent {
    pub id: i64,
    pub conversation_id: Uuid,
    pub event_type: String,
    pub payload: serde_json::Value,
    pub step_id: Option<String>,
    pub step_seq: Option<i32>,
    pub created_at: OffsetDateTime,
}

/// A conversation with aggregate counts for list views.
pub struct ConversationSummary {
    pub id: Uuid,
    pub title: Option<String>,
    pub status: String,
    pub model_name: String,
    pub container_status: String,
    pub total_tool_calls: i32,
    pub total_estimated_cost_usd: f32,
    pub query_status: String,
    pub message_count: i64,
    pub artifact_count: i64,
    pub created_at: OffsetDateTime,
    pub last_activity_at: OffsetDateTime,
}

/// A single message within a conversation.
pub struct ConversationMessage {
    pub id: Uuid,
    pub conversation_id: Uuid,
    pub role: String,
    pub content: String,
    pub reasoning_trace: Option<serde_json::Value>,
    pub supporting_data: Option<serde_json::Value>,
    pub prompt_tokens: i32,
    pub completion_tokens: i32,
    pub created_at: OffsetDateTime,
}

/// An artifact generated during a conversation (metadata — file lives in S3).
pub struct ConversationArtifact {
    pub id: Uuid,
    pub conversation_id: Uuid,
    pub message_id: Option<Uuid>,
    pub artifact_key: String,
    pub display_name: String,
    pub content_type: Option<String>,
    pub size_bytes: i64,
    pub created_at: OffsetDateTime,
}

/// Parameters for creating a new conversation.
pub struct CreateConversationParams<'a> {
    pub user_id: Uuid,
    pub title: Option<&'a str>,
    pub model_name: &'a str,
}

/// Parameters for adding a message to a conversation.
pub struct CreateMessageParams<'a> {
    pub conversation_id: Uuid,
    pub role: &'a str,
    pub content: &'a str,
    pub reasoning_trace: Option<&'a serde_json::Value>,
    pub supporting_data: Option<&'a serde_json::Value>,
    pub prompt_tokens: i32,
    pub completion_tokens: i32,
}

/// Parameters for recording a conversation artifact.
pub struct CreateArtifactParams<'a> {
    pub conversation_id: Uuid,
    pub message_id: Option<Uuid>,
    pub artifact_key: &'a str,
    pub display_name: &'a str,
    pub content_type: Option<&'a str>,
    pub size_bytes: i64,
}

impl ReasoningRepo {
    // -----------------------------------------------------------------------
    // Conversations
    // -----------------------------------------------------------------------

    /// Create a new conversation, returning its ID.
    pub async fn create_conversation(
        &self,
        params: &CreateConversationParams<'_>,
    ) -> Result<Conversation, Error> {
        let row = sqlx::query_as!(
            Conversation,
            r#"
            INSERT INTO reasoning.conversations (user_id, title, model_name)
            VALUES ($1, $2, $3)
            RETURNING id, user_id, title, status, model_name,
                      container_pod_name, container_status, opencode_session_id,
                      total_tool_calls, total_prompt_tokens, total_completion_tokens,
                      total_estimated_cost_usd, query_status, created_at, last_activity_at
            "#,
            params.user_id,
            params.title,
            params.model_name,
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(row)
    }

    /// Get a conversation by ID.
    pub async fn get_conversation(&self, id: Uuid) -> Result<Option<Conversation>, Error> {
        let row = sqlx::query_as!(
            Conversation,
            r#"
            SELECT id, user_id, title, status, model_name,
                   container_pod_name, container_status, opencode_session_id,
                   total_tool_calls, total_prompt_tokens, total_completion_tokens,
                   total_estimated_cost_usd, query_status, created_at, last_activity_at
            FROM reasoning.conversations
            WHERE id = $1
            "#,
            id,
        )
        .fetch_optional(&self.pool)
        .await?;
        Ok(row)
    }

    /// Check if a conversation exists by ID.
    pub async fn conversation_exists(&self, id: Uuid) -> Result<bool, Error> {
        let row = sqlx::query!(
            r#"SELECT EXISTS(SELECT 1 FROM reasoning.conversations WHERE id = $1) as "exists!""#,
            id,
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(row.exists)
    }

    /// List conversations for a user, newest first, with message/artifact counts.
    pub async fn list_conversations(
        &self,
        user_id: Uuid,
        limit: i64,
        offset: i64,
    ) -> Result<(Vec<ConversationSummary>, i64), Error> {
        let (rows, total) = tokio::try_join!(
            async {
                sqlx::query_as!(
                    ConversationSummary,
                    r#"
                    SELECT c.id, c.title, c.status, c.model_name, c.container_status,
                           c.total_tool_calls, c.total_estimated_cost_usd,
                           c.query_status, c.created_at, c.last_activity_at,
                           (SELECT COUNT(*) FROM reasoning.conversation_messages m
                            WHERE m.conversation_id = c.id) AS "message_count!",
                           (SELECT COUNT(*) FROM reasoning.conversation_artifacts a
                            WHERE a.conversation_id = c.id) AS "artifact_count!"
                    FROM reasoning.conversations c
                    WHERE c.user_id = $1
                    ORDER BY c.last_activity_at DESC
                    LIMIT $2 OFFSET $3
                    "#,
                    user_id,
                    limit,
                    offset,
                )
                .fetch_all(&self.pool)
                .await
                .map_err(Error::from)
            },
            async {
                let (count,): (i64,) = sqlx::query_as(
                    "SELECT COUNT(*) FROM reasoning.conversations WHERE user_id = $1",
                )
                .bind(user_id)
                .fetch_one(&self.pool)
                .await
                .map_err(Error::from)?;
                Ok::<_, Error>(count)
            },
        )?;
        Ok((rows, total))
    }

    /// Update the container lifecycle fields on a conversation.
    pub async fn update_container_status(
        &self,
        conversation_id: Uuid,
        pod_name: Option<&str>,
        container_status: &str,
        opencode_session_id: Option<&str>,
    ) -> Result<(), Error> {
        sqlx::query!(
            r#"
            UPDATE reasoning.conversations
            SET container_pod_name = COALESCE($2, container_pod_name),
                container_status = $3,
                opencode_session_id = COALESCE($4, opencode_session_id),
                last_activity_at = now()
            WHERE id = $1
            "#,
            conversation_id,
            pod_name,
            container_status,
            opencode_session_id,
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Update conversation totals after a completed turn.
    pub async fn update_conversation_totals(
        &self,
        conversation_id: Uuid,
        tool_calls: i32,
        prompt_tokens: i32,
        completion_tokens: i32,
        estimated_cost_usd: f32,
    ) -> Result<(), Error> {
        sqlx::query!(
            r#"
            UPDATE reasoning.conversations
            SET total_tool_calls = total_tool_calls + $2,
                total_prompt_tokens = total_prompt_tokens + $3,
                total_completion_tokens = total_completion_tokens + $4,
                total_estimated_cost_usd = total_estimated_cost_usd + $5,
                last_activity_at = now()
            WHERE id = $1
            "#,
            conversation_id,
            tool_calls,
            prompt_tokens,
            completion_tokens,
            estimated_cost_usd,
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Conversation events (ephemeral streaming log)
    // -----------------------------------------------------------------------

    /// Append an event to the conversation event log.
    pub async fn append_event(
        &self,
        conversation_id: Uuid,
        event_type: &str,
        payload: &serde_json::Value,
        step_id: Option<&str>,
        step_seq: Option<i32>,
    ) -> Result<ConversationEvent, Error> {
        let row = sqlx::query_as!(
            ConversationEvent,
            r#"
            INSERT INTO reasoning.conversation_events
              (conversation_id, event_type, payload, step_id, step_seq)
            VALUES ($1, $2, $3, $4, $5)
            RETURNING id, conversation_id, event_type, payload, step_id, step_seq, created_at
            "#,
            conversation_id,
            event_type,
            payload,
            step_id,
            step_seq,
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(row)
    }

    /// Poll events after a given cursor (BIGINT id), ordered by insertion.
    pub async fn poll_events(
        &self,
        conversation_id: Uuid,
        after_id: i64,
    ) -> Result<Vec<ConversationEvent>, Error> {
        let rows = sqlx::query_as!(
            ConversationEvent,
            r#"
            SELECT id, conversation_id, event_type, payload, step_id, step_seq, created_at
            FROM reasoning.conversation_events
            WHERE conversation_id = $1 AND id > $2
            ORDER BY id
            "#,
            conversation_id,
            after_id,
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    /// Delete all events for a conversation (cleanup after completion).
    pub async fn delete_events(&self, conversation_id: Uuid) -> Result<u64, Error> {
        let result = sqlx::query!(
            "DELETE FROM reasoning.conversation_events WHERE conversation_id = $1",
            conversation_id,
        )
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected())
    }

    /// Return all events for a conversation, ordered by insertion.
    /// Used by the worker to derive the final reasoning trace.
    pub async fn get_all_events(
        &self,
        conversation_id: Uuid,
    ) -> Result<Vec<ConversationEvent>, Error> {
        self.poll_events(conversation_id, 0).await
    }

    /// Delete stale events for conversations that are no longer active.
    /// Used as a safety net for cases where the worker crashes before cleanup.
    pub async fn cleanup_stale_events(&self, max_age_hours: i32) -> Result<u64, Error> {
        let result = sqlx::query!(
            r#"
            DELETE FROM reasoning.conversation_events
            WHERE conversation_id IN (
                SELECT id FROM reasoning.conversations
                WHERE query_status IN ('completed', 'failed', 'cancelled')
            )
            AND created_at < now() - make_interval(hours => $1)
            "#,
            max_age_hours,
        )
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected())
    }

    /// Delete a conversation and all related data (messages, artifacts, events).
    /// Returns the container pod name if one was associated (for reaping).
    pub async fn delete_conversation(
        &self,
        conversation_id: Uuid,
        user_id: Uuid,
    ) -> Result<Option<String>, Error> {
        // Fetch pod name before deletion so caller can reap the container.
        let pod_name: Option<String> = sqlx::query_scalar!(
            r#"
            SELECT container_pod_name
            FROM reasoning.conversations
            WHERE id = $1 AND user_id = $2
            "#,
            conversation_id,
            user_id,
        )
        .fetch_optional(&self.pool)
        .await?
        .flatten();

        // Cascade: events → artifacts → messages → conversation.
        sqlx::query!(
            "DELETE FROM reasoning.conversation_events WHERE conversation_id = $1",
            conversation_id,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query!(
            "DELETE FROM reasoning.conversation_artifacts WHERE conversation_id = $1",
            conversation_id,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query!(
            "DELETE FROM reasoning.conversation_messages WHERE conversation_id = $1",
            conversation_id,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query!(
            "DELETE FROM reasoning.conversations WHERE id = $1 AND user_id = $2",
            conversation_id,
            user_id,
        )
        .execute(&self.pool)
        .await?;

        Ok(pod_name)
    }

    /// Rename a conversation (set its title).
    pub async fn rename_conversation(
        &self,
        conversation_id: Uuid,
        user_id: Uuid,
        title: &str,
    ) -> Result<(), Error> {
        sqlx::query!(
            r#"
            UPDATE reasoning.conversations
            SET title = $3
            WHERE id = $1 AND user_id = $2
            "#,
            conversation_id,
            user_id,
            title,
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Update the query lifecycle status on a conversation.
    pub async fn update_query_status(
        &self,
        conversation_id: Uuid,
        query_status: &str,
    ) -> Result<(), Error> {
        sqlx::query!(
            r#"
            UPDATE reasoning.conversations
            SET query_status = $2, last_activity_at = now()
            WHERE id = $1
            "#,
            conversation_id,
            query_status,
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Conversation messages
    // -----------------------------------------------------------------------

    /// Add a message to a conversation.
    pub async fn create_message(
        &self,
        params: &CreateMessageParams<'_>,
    ) -> Result<ConversationMessage, Error> {
        let row = sqlx::query_as!(
            ConversationMessage,
            r#"
            INSERT INTO reasoning.conversation_messages
                (conversation_id, role, content, reasoning_trace, supporting_data,
                 prompt_tokens, completion_tokens)
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            RETURNING id, conversation_id, role, content, reasoning_trace,
                      supporting_data, prompt_tokens, completion_tokens, created_at
            "#,
            params.conversation_id,
            params.role,
            params.content,
            params.reasoning_trace,
            params.supporting_data,
            params.prompt_tokens,
            params.completion_tokens,
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(row)
    }

    /// List messages in a conversation, oldest first.
    pub async fn list_messages(
        &self,
        conversation_id: Uuid,
    ) -> Result<Vec<ConversationMessage>, Error> {
        let rows = sqlx::query_as!(
            ConversationMessage,
            r#"
            SELECT id, conversation_id, role, content, reasoning_trace,
                   supporting_data, prompt_tokens, completion_tokens, created_at
            FROM reasoning.conversation_messages
            WHERE conversation_id = $1
            ORDER BY created_at ASC
            "#,
            conversation_id,
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    // -----------------------------------------------------------------------
    // Conversation artifacts
    // -----------------------------------------------------------------------

    /// Record an artifact generated during a conversation.
    pub async fn create_artifact(
        &self,
        params: &CreateArtifactParams<'_>,
    ) -> Result<ConversationArtifact, Error> {
        let row = sqlx::query_as!(
            ConversationArtifact,
            r#"
            INSERT INTO reasoning.conversation_artifacts
                (conversation_id, message_id, artifact_key, display_name,
                 content_type, size_bytes)
            VALUES ($1, $2, $3, $4, $5, $6)
            ON CONFLICT (conversation_id, artifact_key) DO UPDATE
                SET display_name = EXCLUDED.display_name,
                    size_bytes = EXCLUDED.size_bytes
            RETURNING id, conversation_id, message_id, artifact_key, display_name,
                      content_type, size_bytes, created_at
            "#,
            params.conversation_id,
            params.message_id,
            params.artifact_key,
            params.display_name,
            params.content_type,
            params.size_bytes,
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(row)
    }

    /// List artifacts for a conversation.
    pub async fn list_artifacts(
        &self,
        conversation_id: Uuid,
    ) -> Result<Vec<ConversationArtifact>, Error> {
        let rows = sqlx::query_as!(
            ConversationArtifact,
            r#"
            SELECT id, conversation_id, message_id, artifact_key, display_name,
                   content_type, size_bytes, created_at
            FROM reasoning.conversation_artifacts
            WHERE conversation_id = $1
            ORDER BY created_at ASC
            "#,
            conversation_id,
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    /// Get a single artifact by ID (for download URL generation).
    pub async fn get_artifact(&self, id: Uuid) -> Result<Option<ConversationArtifact>, Error> {
        let row = sqlx::query_as!(
            ConversationArtifact,
            r#"
            SELECT id, conversation_id, message_id, artifact_key, display_name,
                   content_type, size_bytes, created_at
            FROM reasoning.conversation_artifacts
            WHERE id = $1
            "#,
            id,
        )
        .fetch_optional(&self.pool)
        .await?;
        Ok(row)
    }

    // -----------------------------------------------------------------------
    // Backup / export
    // -----------------------------------------------------------------------

    /// Count all conversations (for backup manifest).
    pub async fn count_conversations(&self) -> Result<i64, Error> {
        sqlx::query_scalar!(r#"SELECT count(*) as "count!: i64" FROM reasoning.conversations"#,)
            .fetch_one(&self.pool)
            .await
            .map_err(Error::from)
    }

    /// Export all conversations as JSON values for backup.
    pub async fn export_conversations(&self) -> Result<Vec<serde_json::Value>, Error> {
        let rows: Vec<Conversation> = sqlx::query_as!(
            Conversation,
            r#"
            SELECT id, user_id, title, status, model_name,
                   container_pod_name, container_status, opencode_session_id,
                   total_tool_calls, total_prompt_tokens, total_completion_tokens,
                   total_estimated_cost_usd, query_status, created_at, last_activity_at
            FROM reasoning.conversations
            ORDER BY created_at
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .iter()
            .map(|c| {
                serde_json::json!({
                    "id": c.id,
                    "user_id": c.user_id,
                    "title": c.title,
                    "status": c.status,
                    "model_name": c.model_name,
                    "container_pod_name": c.container_pod_name,
                    "container_status": c.container_status,
                    "opencode_session_id": c.opencode_session_id,
                    "total_tool_calls": c.total_tool_calls,
                    "total_prompt_tokens": c.total_prompt_tokens,
                    "total_completion_tokens": c.total_completion_tokens,
                    "total_estimated_cost_usd": c.total_estimated_cost_usd,
                    "query_status": c.query_status,
                    "created_at": c.created_at.to_string(),
                    "last_activity_at": c.last_activity_at.to_string(),
                })
            })
            .collect())
    }

    /// Export all conversation messages as JSON values for backup.
    pub async fn export_conversation_messages(&self) -> Result<Vec<serde_json::Value>, Error> {
        let rows: Vec<ConversationMessage> = sqlx::query_as!(
            ConversationMessage,
            r#"
            SELECT id, conversation_id, role, content,
                   reasoning_trace, supporting_data,
                   prompt_tokens, completion_tokens, created_at
            FROM reasoning.conversation_messages
            ORDER BY created_at
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .iter()
            .map(|m| {
                serde_json::json!({
                    "id": m.id,
                    "conversation_id": m.conversation_id,
                    "role": m.role,
                    "content": m.content,
                    "reasoning_trace": m.reasoning_trace,
                    "supporting_data": m.supporting_data,
                    "prompt_tokens": m.prompt_tokens,
                    "completion_tokens": m.completion_tokens,
                    "created_at": m.created_at.to_string(),
                })
            })
            .collect())
    }

    /// Export all conversation artifacts as JSON values for backup.
    pub async fn export_conversation_artifacts(&self) -> Result<Vec<serde_json::Value>, Error> {
        let rows: Vec<ConversationArtifact> = sqlx::query_as!(
            ConversationArtifact,
            r#"
            SELECT id, conversation_id, message_id, artifact_key,
                   display_name, content_type, size_bytes, created_at
            FROM reasoning.conversation_artifacts
            ORDER BY created_at
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .iter()
            .map(|a| {
                serde_json::json!({
                    "id": a.id,
                    "conversation_id": a.conversation_id,
                    "message_id": a.message_id,
                    "artifact_key": a.artifact_key,
                    "display_name": a.display_name,
                    "content_type": a.content_type,
                    "size_bytes": a.size_bytes,
                    "created_at": a.created_at.to_string(),
                })
            })
            .collect())
    }
}
