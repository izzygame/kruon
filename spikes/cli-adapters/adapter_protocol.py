"""Adapter Protocol — ABC defining the kruon deep-adapter contract.

Per plan section 6, every deep adapter (Codex, Claude, …) must implement
these 12 methods.  The protocol is a *structural* contract expressed via
``abc.ABC`` + ``@abstractmethod`` so that static type checkers and runtime
enforcement catch missing methods early.

The protocol intentionally does NOT depend on any CLI-specific knowledge:
all adapter-specific logic lives in the concrete implementation.
"""

from __future__ import annotations

import abc
from dataclasses import dataclass, field
from typing import Any, AsyncIterator, Dict, Iterator, List, Literal, Optional


AuthState = Literal["authenticated", "unauthenticated", "unknown"]
ApprovalDecisionKind = Literal["approved", "denied", "modified"]
TerminalState = Literal[
    "completed", "failed", "cancelled", "forced_stop_required", "unknown"
]


# --------------------------------------------------------------------------- #
# Shared value objects (lightweight, no CLI dependency)
# --------------------------------------------------------------------------- #


@dataclass
class ToolIdentity:
    name: str
    version: Optional[str] = None
    auth_state: AuthState = "unknown"


@dataclass
class AdapterSession:
    session_id: str
    adapter: str
    pid: Optional[int] = None
    started_at: Optional[str] = None
    metadata: Dict[str, Any] = field(default_factory=dict)


@dataclass
class ApprovalDecision:
    session_id: str
    event_id: str
    decision: ApprovalDecisionKind
    modified_params: Optional[Dict[str, Any]] = None
    reason: Optional[str] = None


@dataclass
class ArtifactCandidate:
    path: str
    kind: Optional[str] = None
    # Fail closed: callers must positively establish workspace containment.
    in_workspace: bool = False
    size_bytes: Optional[int] = None
    checksum: Optional[str] = None


@dataclass
class ObservedTerminalState:
    terminal_state: TerminalState
    exit_code: Optional[int] = None
    reconciled: bool = False


@dataclass
class RedactedDiagnosticBundle:
    adapter: str
    session_id: str
    event_count: int
    parse_error_count: int
    degraded_count: int
    terminal_state: Optional[TerminalState] = None
    stdout_truncated: Optional[str] = None
    stderr_truncated: Optional[str] = None
    errors: List[str] = field(default_factory=list)


# --------------------------------------------------------------------------- #
# The Adapter Protocol
# --------------------------------------------------------------------------- #


class AdapterProtocol(abc.ABC):
    """Every kruon deep adapter must implement this protocol.

    Lifecycle
    ---------
    1. ``probe()``              — discover & authenticate
    2. ``capabilities()``       — declare what this version can do
    3. ``prepare()``            — freeze a launch plan (fingerprinted)
    4. ``start()``              — spawn the process
    5. ``send_input()``         — feed stdin
    6. ``stream_events()``      — consume normalized events
    7. ``respond_approval()``   — answer an approval request
    8. ``cancel()``             — terminate the run
    9. ``resume()``             — re-attach to a prior session
    10. ``collect_artifacts()``  — gather produced files / diffs / reports
    11. ``reconcile()``         — determine final terminal state
    12. ``diagnostics()``       — produce a redacted diagnostic bundle
    """

    # -- Discovery -----------------------------------------------------------

    @abc.abstractmethod
    def probe(self) -> ToolIdentity:
        """Discover the tool, its version, and authentication state.

        Must be side-effect-free beyond reading --version / --help.
        """

    # -- Capabilities --------------------------------------------------------

    @abc.abstractmethod
    def capabilities(self, version: Optional[str] = None) -> Dict[str, Any]:
        """Return the capability manifest for a given version.

        The returned dict must validate against
        ``capability_manifest.schema.json``.
        """

    # -- Lifecycle -----------------------------------------------------------

    @abc.abstractmethod
    def prepare(
        self,
        task: str,
        workspace: str,
        policy: Optional[Dict[str, Any]] = None,
    ) -> Dict[str, Any]:
        """Freeze a fingerprinted launch plan.

        Returns a dict with at least: ``adapter``, ``tool_name``,
        ``workspace``, ``task``, ``approval_mode``, ``argv``,
        ``fingerprint``.
        """

    @abc.abstractmethod
    def start(self, launch_plan: Dict[str, Any]) -> AdapterSession:
        """Spawn the adapter process per the frozen launch plan."""

    @abc.abstractmethod
    def send_input(self, session: AdapterSession, input_data: str) -> None:
        """Write input to the running adapter's stdin."""

    @abc.abstractmethod
    def stream_events(
        self, session: AdapterSession
    ) -> Iterator[Dict[str, Any]]:
        """Yield normalized event dicts from the running adapter."""

    # -- Approval ------------------------------------------------------------

    @abc.abstractmethod
    def respond_approval(
        self, session: AdapterSession, decision: ApprovalDecision
    ) -> None:
        """Send an approval / denial / modification decision upstream."""

    # -- Cancellation & Resume -----------------------------------------------

    @abc.abstractmethod
    def cancel(self, session: AdapterSession, deadline_seconds: float = 10.0) -> None:
        """Request cancellation of the run.

        Must cover the process tree, PTY, and orphan detection.
        """

    @abc.abstractmethod
    def resume(self, session_ref: str) -> Optional[AdapterSession]:
        """Re-attach to a prior session by reference.

        Returns ``None`` if the session cannot be resumed.
        """

    # -- Artifacts & Reconciliation ------------------------------------------

    @abc.abstractmethod
    def collect_artifacts(
        self, session: AdapterSession
    ) -> List[ArtifactCandidate]:
        """Gather produced files, diffs, test results, and reports."""

    @abc.abstractmethod
    def reconcile(self, session: AdapterSession) -> ObservedTerminalState:
        """Determine the final terminal state of a finished or cancelled run.

        Must never coerce ``unknown`` to ``completed``.
        """

    # -- Diagnostics ---------------------------------------------------------

    @abc.abstractmethod
    def diagnostics(self, session: AdapterSession) -> RedactedDiagnosticBundle:
        """Produce a redacted diagnostic bundle for debugging / support."""


# --------------------------------------------------------------------------- #
# Async variant (for future Tauri / async runtimes)
# --------------------------------------------------------------------------- #


class AsyncAdapterProtocol(abc.ABC):
    """Async version of the adapter protocol for Tauri / asyncio runtimes."""

    @abc.abstractmethod
    async def probe(self) -> ToolIdentity:
        ...

    @abc.abstractmethod
    async def capabilities(self, version: Optional[str] = None) -> Dict[str, Any]:
        ...

    @abc.abstractmethod
    async def prepare(
        self,
        task: str,
        workspace: str,
        policy: Optional[Dict[str, Any]] = None,
    ) -> Dict[str, Any]:
        ...

    @abc.abstractmethod
    async def start(self, launch_plan: Dict[str, Any]) -> AdapterSession:
        ...

    @abc.abstractmethod
    async def send_input(self, session: AdapterSession, input_data: str) -> None:
        ...

    @abc.abstractmethod
    def stream_events(
        self, session: AdapterSession
    ) -> AsyncIterator[Dict[str, Any]]:
        ...

    @abc.abstractmethod
    async def respond_approval(
        self, session: AdapterSession, decision: ApprovalDecision
    ) -> None:
        ...

    @abc.abstractmethod
    async def cancel(self, session: AdapterSession, deadline_seconds: float = 10.0) -> None:
        ...

    @abc.abstractmethod
    async def resume(self, session_ref: str) -> Optional[AdapterSession]:
        ...

    @abc.abstractmethod
    async def collect_artifacts(
        self, session: AdapterSession
    ) -> List[ArtifactCandidate]:
        ...

    @abc.abstractmethod
    async def reconcile(self, session: AdapterSession) -> ObservedTerminalState:
        ...

    @abc.abstractmethod
    async def diagnostics(self, session: AdapterSession) -> RedactedDiagnosticBundle:
        ...
