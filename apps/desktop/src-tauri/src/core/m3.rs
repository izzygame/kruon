use serde::{Deserialize, Serialize};

use super::domain::{AdapterKind, EventEnvelope, EventPhase, RunSnapshot, RunStatus};
use super::m2::TaskReviewStatus;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorldAgentState {
    Idle,
    Planning,
    Running,
    WaitingApproval,
    Blocked,
    Reviewing,
    Completed,
    Sleeping,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorldStationProjection {
    pub station_id: String,
    pub adapter: AdapterKind,
    pub run_id: Option<String>,
    pub state: WorldAgentState,
    pub run_status: Option<RunStatus>,
    pub source_sequence: u64,
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorldSnapshot {
    pub generated_at: String,
    pub stations: Vec<WorldStationProjection>,
}

pub fn project_world_station(
    adapter: AdapterKind,
    run: Option<&RunSnapshot>,
    events: &[EventEnvelope],
    review: Option<TaskReviewStatus>,
) -> WorldStationProjection {
    let Some(run) = run else {
        return WorldStationProjection {
            station_id: format!("{}-desk", adapter.as_str()),
            adapter,
            run_id: None,
            state: WorldAgentState::Sleeping,
            run_status: None,
            source_sequence: 0,
            updated_at: None,
        };
    };

    WorldStationProjection {
        station_id: format!("{}-desk", adapter.as_str()),
        adapter,
        run_id: Some(run.run_id.clone()),
        state: project_state(run, events.last().map(|event| event.phase), review),
        run_status: Some(run.status.clone()),
        source_sequence: events.last().map(|event| event.sequence).unwrap_or(0),
        updated_at: Some(run.updated_at.clone()),
    }
}

fn project_state(
    run: &RunSnapshot,
    latest_phase: Option<EventPhase>,
    review: Option<TaskReviewStatus>,
) -> WorldAgentState {
    match &run.status {
        RunStatus::Pending => WorldAgentState::Idle,
        RunStatus::Planning => WorldAgentState::Planning,
        RunStatus::WaitingApproval => WorldAgentState::WaitingApproval,
        RunStatus::Cancelling
        | RunStatus::Failed
        | RunStatus::Cancelled
        | RunStatus::ForcedStopRequired
        | RunStatus::Uncertain => WorldAgentState::Blocked,
        RunStatus::Completed => match review {
            Some(TaskReviewStatus::Accepted) => WorldAgentState::Completed,
            Some(TaskReviewStatus::Returned) => WorldAgentState::Blocked,
            None => WorldAgentState::Reviewing,
        },
        RunStatus::Running => match latest_phase {
            Some(EventPhase::Setup | EventPhase::Planning) | None => WorldAgentState::Planning,
            Some(EventPhase::WaitingApproval) => WorldAgentState::WaitingApproval,
            Some(EventPhase::Degraded | EventPhase::Uncertain | EventPhase::Cancelling) => {
                WorldAgentState::Blocked
            }
            Some(EventPhase::Terminal) => WorldAgentState::Blocked,
            Some(
                EventPhase::Running
                | EventPhase::ToolCall
                | EventPhase::ApprovalDecision
                | EventPhase::Artifact,
            ) => WorldAgentState::Running,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::domain::TerminalState;

    fn run(status: RunStatus) -> RunSnapshot {
        RunSnapshot {
            run_id: "run-1".into(),
            adapter: AdapterKind::Codex,
            workspace_root: "redacted".into(),
            working_directory: "redacted".into(),
            policy_id: Some("read-only".into()),
            status,
            terminal_state: None,
            created_at: "2026-07-16T00:00:00Z".into(),
            updated_at: "2026-07-16T00:01:00Z".into(),
            last_sequence: 0,
            prompt_hash: "hash".into(),
            launch_fingerprint: "fingerprint".into(),
            pid: None,
            pgid: None,
        }
    }

    fn event(phase: EventPhase, sequence: u64) -> EventEnvelope {
        EventEnvelope::new(
            "run-1",
            sequence,
            "fixture",
            phase,
            None,
            serde_json::json!({}),
        )
    }

    #[test]
    fn maps_all_frozen_world_states_without_an_independent_store() {
        assert_eq!(
            project_world_station(AdapterKind::Claude, None, &[], None).state,
            WorldAgentState::Sleeping
        );
        assert_eq!(
            project_world_station(
                AdapterKind::Codex,
                Some(&run(RunStatus::Pending)),
                &[],
                None
            )
            .state,
            WorldAgentState::Idle
        );
        assert_eq!(
            project_world_station(
                AdapterKind::Codex,
                Some(&run(RunStatus::Planning)),
                &[],
                None,
            )
            .state,
            WorldAgentState::Planning
        );
        assert_eq!(
            project_world_station(
                AdapterKind::Codex,
                Some(&run(RunStatus::Running)),
                &[event(EventPhase::ToolCall, 1)],
                None,
            )
            .state,
            WorldAgentState::Running
        );
        assert_eq!(
            project_world_station(
                AdapterKind::Codex,
                Some(&run(RunStatus::WaitingApproval)),
                &[],
                None,
            )
            .state,
            WorldAgentState::WaitingApproval
        );
        assert_eq!(
            project_world_station(AdapterKind::Codex, Some(&run(RunStatus::Failed)), &[], None,)
                .state,
            WorldAgentState::Blocked
        );
        assert_eq!(
            project_world_station(
                AdapterKind::Codex,
                Some(&run(RunStatus::Completed)),
                &[],
                None,
            )
            .state,
            WorldAgentState::Reviewing
        );
        assert_eq!(
            project_world_station(
                AdapterKind::Codex,
                Some(&run(RunStatus::Completed)),
                &[],
                Some(TaskReviewStatus::Accepted),
            )
            .state,
            WorldAgentState::Completed
        );
    }

    #[test]
    fn projection_preserves_the_replayed_sequence_and_terminal_truth() {
        let mut completed = run(RunStatus::Completed);
        completed.last_sequence = 2;
        completed.terminal_state = Some(TerminalState::Completed);
        let events = vec![
            event(EventPhase::Running, 1),
            EventEnvelope::new(
                "run-1",
                2,
                "run.terminal",
                EventPhase::Terminal,
                Some(TerminalState::Completed),
                serde_json::json!({}),
            ),
        ];

        let projection = project_world_station(
            AdapterKind::Codex,
            Some(&completed),
            &events,
            Some(TaskReviewStatus::Accepted),
        );
        assert_eq!(projection.source_sequence, completed.last_sequence);
        assert_eq!(projection.run_status, Some(RunStatus::Completed));
        assert_eq!(projection.state, WorldAgentState::Completed);
    }
}
