#![allow(non_snake_case)]

pub mod error;
mod macros;
mod typed_envelope;

use std::cmp;
use std::iter;
use std::mem;
use std::time::Duration;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

pub use error::*;
pub use prost::DecodeError;
pub use prost::Message;
pub use typed_envelope::*;

mod generated {
    #![allow(clippy::missing_const_for_fn)]
    include!(concat!(env!("OUT_DIR"), "/ed.messages.rs"));
}
pub use generated::*;

pub const REMOTE_SERVER_PEER_ID: PeerId = PeerId { owner_id: 0, id: 0 };
pub const REMOTE_SERVER_PROJECT_ID: u64 = 0;

impl Envelope {
    #[inline(never)]
    pub fn decode_from_slice(buffer: &[u8]) -> Result<Self, DecodeError> {
        Self::decode(buffer)
    }

    #[inline(never)]
    pub fn encode_to_buffer(&self, buffer: &mut Vec<u8>) -> Result<(), prost::EncodeError> {
        self.encode(buffer)
    }

    #[inline(never)]
    pub fn encoded_size(&self) -> usize {
        self.encoded_len()
    }
}

messages!(
    (Ack, Foreground),
    (AddWorktree, Foreground),
    (AddWorktreeResponse, Foreground),
    (AllocateWorktreeId, Foreground),
    (AllocateWorktreeIdResponse, Foreground),
    (ApplyCodeAction, Background),
    (ApplyCodeActionResponse, Background),
    (ApplyCompletionAdditionalEdits, Background),
    (ApplyCompletionAdditionalEditsResponse, Background),
    (BlameBuffer, Foreground),
    (BlameBufferResponse, Foreground),
    (BufferReloaded, Foreground),
    (BufferSaved, Foreground),
    (CancelLanguageServerWork, Foreground),
    (CloseBuffer, Foreground),
    (Commit, Background),
    (RunGitHook, Background),
    (CopyProjectEntry, Foreground),
    (CreateBufferForPeer, Foreground),
    (CreateImageForPeer, Foreground),
    (CreateFileForPeer, Foreground),
    (CreateProjectEntry, Foreground),
    (DeleteProjectEntry, Foreground),
    (TrashProjectEntry, Foreground),
    (TrashProjectEntryResponse, Foreground),
    (RestoreProjectEntry, Foreground),
    (RestoreProjectEntryResponse, Foreground),
    (DownloadFileByPath, Background),
    (DownloadFileResponse, Background),
    (EndStream, Foreground),
    (Error, Foreground),
    (ExpandProjectEntry, Foreground),
    (ExpandProjectEntryResponse, Foreground),
    (FindSearchCandidates, Background),
    (FlushBufferedMessages, Foreground),
    (ExpandAllForProjectEntry, Foreground),
    (ExpandAllForProjectEntryResponse, Foreground),
    (RescanWorktrees, Foreground),
    (ApplyCodeActionKind, Foreground),
    (ApplyCodeActionKindResponse, Foreground),
    (FormatBuffers, Foreground),
    (FormatBuffersResponse, Foreground),
    (GetCodeActions, Background),
    (GetCodeActionsResponse, Background),
    (GetCompletions, Background),
    (GetCompletionsResponse, Background),
    (GetDeclaration, Background),
    (GetDeclarationResponse, Background),
    (GetDefinition, Background),
    (GetDefinitionResponse, Background),
    (GetDocumentHighlights, Background),
    (GetDocumentHighlightsResponse, Background),
    (GetDocumentSymbols, Background),
    (GetDocumentSymbolsResponse, Background),
    (GetHover, Background),
    (GetHoverResponse, Background),
    (GetPathMetadata, Background),
    (GetPathMetadataResponse, Background),
    (GetProjectSymbols, Background),
    (GetProjectSymbolsResponse, Background),
    (GetReferences, Background),
    (GetReferencesResponse, Background),
    (GetTerminalShell, Background),
    (GetTerminalShellResponse, Background),
    (GetTypeDefinition, Background),
    (GetTypeDefinitionResponse, Background),
    (GetImplementation, Background),
    (GetImplementationResponse, Background),
    (OpenUnstagedDiff, Foreground),
    (OpenUnstagedDiffResponse, Foreground),
    (OpenUncommittedDiff, Foreground),
    (OpenUncommittedDiffResponse, Foreground),
    (GitGetBranches, Background),
    (GitBranchesResponse, Background),
    (Hello, Foreground),
    (HideToast, Background),
    (InlayHints, Background),
    (InlayHintsResponse, Background),
    (SemanticTokens, Background),
    (SemanticTokensResponse, Background),
    (LanguageServerLog, Foreground),
    (LanguageServerPromptRequest, Foreground),
    (LanguageServerPromptResponse, Foreground),
    (LanguageServerShowDocumentRequest, Background),
    (LinkedEditingRange, Background),
    (LinkedEditingRangeResponse, Background),
    (ListRemoteDirectory, Background),
    (ListRemoteDirectoryResponse, Background),
    (LoadCommitDiff, Foreground),
    (LoadCommitDiffResponse, Foreground),
    (LspExtExpandMacro, Background),
    (LspExtExpandMacroResponse, Background),
    (LspExtOpenDocs, Background),
    (LspExtOpenDocsResponse, Background),
    (LspExtRunnables, Background),
    (LspExtRunnablesResponse, Background),
    (LspExtGoToParentModule, Background),
    (LspExtGoToParentModuleResponse, Background),
    (ExecuteLspCommand, Background),
    (ExecuteLspCommandResponse, Background),
    (LspExtCancelFlycheck, Background),
    (LspExtRunFlycheck, Background),
    (LspExtClearFlycheck, Background),
    (LspQuery, Background),
    (LspQueryResponse, Background),
    (OnTypeFormatting, Background),
    (OnTypeFormattingResponse, Background),
    (OpenBufferById, Background),
    (OpenBufferByPath, Background),
    (OpenImageByPath, Background),
    (OpenBufferForSymbol, Background),
    (OpenBufferForSymbolResponse, Background),
    (OpenBufferResponse, Background),
    (OpenImageResponse, Background),
    (OpenCommitMessageBuffer, Background),
    (OpenNewBuffer, Foreground),
    (OpenServerSettings, Foreground),
    (PerformRename, Background),
    (PerformRenameResponse, Background),
    (Ping, Foreground),
    (PrepareRename, Background),
    (PrepareRenameResponse, Background),
    (ProjectEntryResponse, Foreground),
    (RefreshInlayHints, Background),
    (RefreshSemanticTokens, Background),
    (RefreshDocumentColors, Background),
    (RefreshDocumentLinks, Background),
    (RefreshDocumentHighlights, Background),
    (RefreshFoldingRanges, Background),
    (RefreshDocumentSymbols, Background),
    (RegisterBufferWithLanguageServers, Background),
    (ReloadBuffers, Foreground),
    (ReloadBuffersResponse, Foreground),
    (RemoveWorktree, Foreground),
    (RenameProjectEntry, Foreground),
    (ResolveCompletionDocumentation, Background),
    (ResolveCompletionDocumentationResponse, Background),
    (ResolveInlayHint, Background),
    (ResolveInlayHintResponse, Background),
    (GetDocumentColor, Background),
    (GetDocumentColorResponse, Background),
    (GetColorPresentation, Background),
    (GetColorPresentationResponse, Background),
    (GetDocumentLinks, Background),
    (GetDocumentLinksResponse, Background),
    (ResolveDocumentLink, Background),
    (ResolveDocumentLinkResponse, Background),
    (GetFoldingRanges, Background),
    (GetFoldingRangesResponse, Background),
    (RefreshCodeLens, Background),
    (GetCodeLens, Background),
    (GetCodeLensResponse, Background),
    (ResolveCodeAction, Background),
    (ResolveCodeActionResponse, Background),
    (RestartLanguageServers, Foreground),
    (StopLanguageServers, Background),
    (SaveBuffer, Foreground),
    (ShutdownRemoteServer, Foreground),
    (Stage, Background),
    (StartLanguageServer, Foreground),
    (SynchronizeBuffers, Foreground),
    (SynchronizeBuffersResponse, Foreground),
    (TaskContext, Background),
    (TaskContextForLocation, Background),
    (Test, Foreground),
    (Toast, Background),
    (Unstage, Background),
    (Stash, Background),
    (StashPop, Background),
    (StashApply, Background),
    (StashDrop, Background),
    (UpdateBuffer, Foreground),
    (UpdateBufferFile, Foreground),
    (UpdateDiagnosticSummary, Foreground),
    (UpdateDiffBases, Foreground),
    (UpdateGitBranch, Background),
    (UpdateLanguageServer, Foreground),
    (UpdateProject, Foreground),
    (UpdateWorktree, Foreground),
    (UpdateRepository, Foreground),
    (RemoveRepository, Foreground),
    (GitReset, Background),
    (GitDeleteBranch, Background),
    (GitCheckoutFiles, Background),
    (GitAddPathToGitignore, Background),
    (GitAddPathToGitInfoExclude, Background),
    (GitShow, Background),
    (GitCommitDetails, Background),
    (SetIndexText, Background),
    (Push, Background),
    (Fetch, Background),
    (GetRemotes, Background),
    (GetRemotesResponse, Background),
    (Pull, Background),
    (RemoteMessageResponse, Background),
    (GitCreateRemote, Background),
    (GitRemoveRemote, Background),
    (GitCreateBranch, Background),
    (GitChangeBranch, Background),
    (GitRenameBranch, Background),
    (TrustWorktrees, Background),
    (RestrictWorktrees, Background),
    (CheckForPushedCommits, Background),
    (CheckForPushedCommitsResponse, Background),
    (GitDiff, Background),
    (GitDiffResponse, Background),
    (GitInit, Background),
    (GetDocumentDiagnostics, Background),
    (GetDocumentDiagnosticsResponse, Background),
    (PullWorkspaceDiagnostics, Background),
    (GetDefaultBranch, Background),
    (GetDefaultBranchResponse, Background),
    (GetTreeDiff, Background),
    (GetTreeDiffResponse, Background),
    (GetBlobContent, Background),
    (GetBlobContentResponse, Background),
    (BlameBufferAtRevision, Background),
    (BlameBufferAtRevisionResponse, Background),
    (LoadCommitTemplate, Background),
    (LoadCommitTemplateResponse, Background),
    (GitClone, Background),
    (GitCloneResponse, Background),
    // Reconnect reconciliation must stay ordered with subsequent user toggles.
    (ToggleLspLogs, Foreground),
    (GetDirectoryEnvironment, Background),
    (DirectoryEnvironment, Background),
    (RemoteStarted, Background),
    (GitGetWorktrees, Background),
    (GitGetHeadSha, Background),
    (GitGetHeadShaResponse, Background),
    (GitEditRef, Background),
    (GitRepairWorktrees, Background),
    (GetCommitData, Background),
    (GetCommitDataResponse, Background),
    (GetInitialGraphData, Background),
    (GetInitialGraphDataResponse, Background),
    (SearchCommits, Background),
    (SearchCommitsResponse, Background),
    (GitWorktreesResponse, Background),
    (GitCreateWorktree, Background),
    (GitRemoveWorktree, Background),
    (GitRenameWorktree, Background),
    (GitWorktreeCreatedAt, Background),
    (GitWorktreeCreatedAtResponse, Background),
    (FindSearchCandidatesChunk, Background),
    (FindSearchCandidatesCancelled, Background),
);

request_messages!(
    (AllocateWorktreeId, AllocateWorktreeIdResponse),
    (ApplyCodeAction, ApplyCodeActionResponse),
    (ApplyCompletionAdditionalEdits, ApplyCompletionAdditionalEditsResponse),
    (Commit, Ack),
    (RunGitHook, Ack),
    (CopyProjectEntry, ProjectEntryResponse),
    (CreateProjectEntry, ProjectEntryResponse),
    (DeleteProjectEntry, ProjectEntryResponse),
    (TrashProjectEntry, TrashProjectEntryResponse),
    (RestoreProjectEntry, RestoreProjectEntryResponse),
    (DownloadFileByPath, DownloadFileResponse),
    (ExpandProjectEntry, ExpandProjectEntryResponse),
    (ExpandAllForProjectEntry, ExpandAllForProjectEntryResponse),
    (RescanWorktrees, Ack),
    (ApplyCodeActionKind, ApplyCodeActionKindResponse),
    (FormatBuffers, FormatBuffersResponse),
    (GetCodeActions, GetCodeActionsResponse),
    (GetCompletions, GetCompletionsResponse),
    (GetDefinition, GetDefinitionResponse),
    (GetDeclaration, GetDeclarationResponse),
    (GetImplementation, GetImplementationResponse),
    (GetDocumentHighlights, GetDocumentHighlightsResponse),
    (GetDocumentSymbols, GetDocumentSymbolsResponse),
    (GetHover, GetHoverResponse),
    (GetProjectSymbols, GetProjectSymbolsResponse),
    (GetReferences, GetReferencesResponse),
    (OpenUnstagedDiff, OpenUnstagedDiffResponse),
    (OpenUncommittedDiff, OpenUncommittedDiffResponse),
    (GetTypeDefinition, GetTypeDefinitionResponse),
    (LinkedEditingRange, LinkedEditingRangeResponse),
    (ListRemoteDirectory, ListRemoteDirectoryResponse),
    (InlayHints, InlayHintsResponse),
    (SemanticTokens, SemanticTokensResponse),
    (GetCodeLens, GetCodeLensResponse),
    (LoadCommitDiff, LoadCommitDiffResponse),
    (OnTypeFormatting, OnTypeFormattingResponse),
    (OpenBufferById, OpenBufferResponse),
    (OpenBufferByPath, OpenBufferResponse),
    (OpenImageByPath, OpenImageResponse),
    (OpenBufferForSymbol, OpenBufferForSymbolResponse),
    (OpenCommitMessageBuffer, OpenBufferResponse),
    (OpenNewBuffer, OpenBufferResponse),
    (PerformRename, PerformRenameResponse),
    (Ping, Ack),
    (PrepareRename, PrepareRenameResponse),
    (RefreshInlayHints, Ack),
    (RefreshSemanticTokens, Ack),
    (RefreshCodeLens, Ack),
    (RefreshDocumentColors, Ack),
    (RefreshDocumentLinks, Ack),
    (RefreshDocumentHighlights, Ack),
    (RefreshFoldingRanges, Ack),
    (RefreshDocumentSymbols, Ack),
    (ReloadBuffers, ReloadBuffersResponse),
    (RenameProjectEntry, ProjectEntryResponse),
    (ResolveCompletionDocumentation, ResolveCompletionDocumentationResponse),
    (ResolveInlayHint, ResolveInlayHintResponse),
    (ResolveCodeAction, ResolveCodeActionResponse),
    (GetDocumentColor, GetDocumentColorResponse),
    (GetDocumentLinks, GetDocumentLinksResponse),
    (ResolveDocumentLink, ResolveDocumentLinkResponse),
    (GetFoldingRanges, GetFoldingRangesResponse),
    (GetColorPresentation, GetColorPresentationResponse),
    (SaveBuffer, BufferSaved),
    (Stage, Ack),
    (FindSearchCandidates, Ack),
    (SynchronizeBuffers, SynchronizeBuffersResponse),
    (TaskContextForLocation, TaskContext),
    (Test, Test),
    (Unstage, Ack),
    (Stash, Ack),
    (StashPop, Ack),
    (StashApply, Ack),
    (StashDrop, Ack),
    (UpdateBuffer, Ack),
    (UpdateProject, Ack),
    (UpdateWorktree, Ack),
    (UpdateRepository, Ack),
    (RemoveRepository, Ack),
    (LspExtExpandMacro, LspExtExpandMacroResponse),
    (LspExtOpenDocs, LspExtOpenDocsResponse),
    (LspExtRunnables, LspExtRunnablesResponse),
    (BlameBuffer, BlameBufferResponse),
    (LspQuery, Ack),
    (LspQueryResponse, Ack),
    (RestartLanguageServers, Ack),
    (StopLanguageServers, Ack),
    (LspExtGoToParentModule, LspExtGoToParentModuleResponse),
    (ExecuteLspCommand, ExecuteLspCommandResponse),
    (LspExtCancelFlycheck, Ack),
    (LspExtRunFlycheck, Ack),
    (LspExtClearFlycheck, Ack),
    (AddWorktree, AddWorktreeResponse),
    (ShutdownRemoteServer, Ack),
    (RemoveWorktree, Ack),
    (OpenServerSettings, OpenBufferResponse),
    (FlushBufferedMessages, Ack),
    (LanguageServerPromptRequest, LanguageServerPromptResponse),
    (LanguageServerShowDocumentRequest, Ack),
    (GitGetBranches, GitBranchesResponse),
    (UpdateGitBranch, Ack),
    (GetPathMetadata, GetPathMetadataResponse),
    (CancelLanguageServerWork, Ack),
    (RegisterBufferWithLanguageServers, Ack),
    (GitShow, GitCommitDetails),
    (GitReset, Ack),
    (GitDeleteBranch, Ack),
    (GitCheckoutFiles, Ack),
    (GitAddPathToGitignore, Ack),
    (GitAddPathToGitInfoExclude, Ack),
    (SetIndexText, Ack),
    (Push, RemoteMessageResponse),
    (Fetch, RemoteMessageResponse),
    (GetRemotes, GetRemotesResponse),
    (Pull, RemoteMessageResponse),
    (GitCreateRemote, Ack),
    (GitRemoveRemote, Ack),
    (GitCreateBranch, Ack),
    (GitChangeBranch, Ack),
    (GitRenameBranch, Ack),
    (CheckForPushedCommits, CheckForPushedCommitsResponse),
    (GitDiff, GitDiffResponse),
    (GitInit, Ack),
    (GetDocumentDiagnostics, GetDocumentDiagnosticsResponse),
    (PullWorkspaceDiagnostics, Ack),
    (GetDefaultBranch, GetDefaultBranchResponse),
    (GetBlobContent, GetBlobContentResponse),
    (BlameBufferAtRevision, BlameBufferAtRevisionResponse),
    (LoadCommitTemplate, LoadCommitTemplateResponse),
    (GetTreeDiff, GetTreeDiffResponse),
    (GitClone, GitCloneResponse),
    (GetDirectoryEnvironment, DirectoryEnvironment),
    (GetTerminalShell, GetTerminalShellResponse),
    (RemoteStarted, Ack),
    (GitGetWorktrees, GitWorktreesResponse),
    (GitGetHeadSha, GitGetHeadShaResponse),
    (GitEditRef, Ack),
    (GitRepairWorktrees, Ack),
    (GetCommitData, GetCommitDataResponse),
    (GetInitialGraphData, GetInitialGraphDataResponse),
    (SearchCommits, SearchCommitsResponse),
    (GitCreateWorktree, Ack),
    (GitRemoveWorktree, Ack),
    (GitRenameWorktree, Ack),
    (GitWorktreeCreatedAt, GitWorktreeCreatedAtResponse),
    (TrustWorktrees, Ack),
    (RestrictWorktrees, Ack),
    (FindSearchCandidatesChunk, Ack),
);

lsp_messages!(
    (GetReferences, GetReferencesResponse, true),
    (GetDocumentColor, GetDocumentColorResponse, true),
    (GetFoldingRanges, GetFoldingRangesResponse, true),
    (GetDocumentSymbols, GetDocumentSymbolsResponse, true),
    (GetDocumentLinks, GetDocumentLinksResponse, true),
    (GetHover, GetHoverResponse, true),
    (GetCodeActions, GetCodeActionsResponse, true),
    (GetCodeLens, GetCodeLensResponse, true),
    (GetDocumentDiagnostics, GetDocumentDiagnosticsResponse, true),
    (GetDefinition, GetDefinitionResponse, true),
    (GetDeclaration, GetDeclarationResponse, true),
    (GetTypeDefinition, GetTypeDefinitionResponse, true),
    (GetImplementation, GetImplementationResponse, true),
    (InlayHints, InlayHintsResponse, false),
    (SemanticTokens, SemanticTokensResponse, true),
);

entity_messages!(
    {project_id, UpdateProject},
    AddWorktree,
    AllocateWorktreeId,
    ApplyCodeAction,
    ApplyCompletionAdditionalEdits,
    BlameBuffer,
    BufferReloaded,
    BufferSaved,
    CloseBuffer,
    Commit,
    RunGitHook,
    GetColorPresentation,
    CopyProjectEntry,
    CreateBufferForPeer,
    CreateFileForPeer,
    CreateImageForPeer,
    CreateProjectEntry,
    GetDocumentColor,
    GetDocumentLinks,
    ResolveDocumentLink,
    GetFoldingRanges,
    DeleteProjectEntry,
    TrashProjectEntry,
    RestoreProjectEntry,
    ExpandProjectEntry,
    ExpandAllForProjectEntry,
    RescanWorktrees,
    FindSearchCandidates,
    ApplyCodeActionKind,
    FormatBuffers,
    GetCodeActions,
    GetCodeLens,
    GetCompletions,
    GetDefinition,
    GetDeclaration,
    GetImplementation,
    GetDocumentHighlights,
    GetDocumentSymbols,
    GetHover,
    GetProjectSymbols,
    GetReferences,
    OpenUnstagedDiff,
    OpenUncommittedDiff,
    GetTypeDefinition,
    InlayHints,
    SemanticTokens,
    LinkedEditingRange,
    LoadCommitDiff,
    LspQuery,
    LspQueryResponse,
    RestartLanguageServers,
    StopLanguageServers,
    OnTypeFormatting,
    OpenNewBuffer,
    OpenBufferById,
    OpenBufferByPath,
    OpenImageByPath,
    OpenBufferForSymbol,
    OpenCommitMessageBuffer,
    PerformRename,
    PrepareRename,
    RefreshInlayHints,
    RefreshSemanticTokens,
    RefreshCodeLens,
    RefreshDocumentColors,
    RefreshDocumentLinks,
    RefreshDocumentHighlights,
    RefreshFoldingRanges,
    RefreshDocumentSymbols,
    ReloadBuffers,
    RenameProjectEntry,
    ResolveCompletionDocumentation,
    ResolveInlayHint,
    ResolveCodeAction,
    SaveBuffer,
    Stage,
    StartLanguageServer,
    SynchronizeBuffers,
    TaskContextForLocation,
    Unstage,
    Stash,
    StashPop,
    StashApply,
    StashDrop,
    UpdateBuffer,
    UpdateBufferFile,
    UpdateDiagnosticSummary,
    UpdateDiffBases,
    UpdateLanguageServer,
    UpdateProject,
    UpdateWorktree,
    UpdateRepository,
    RemoveRepository,
    LspExtExpandMacro,
    LspExtOpenDocs,
    LspExtRunnables,
    LspExtGoToParentModule,
    ExecuteLspCommand,
    LspExtCancelFlycheck,
    LspExtRunFlycheck,
    LspExtClearFlycheck,
    LanguageServerLog,
    Toast,
    HideToast,
    OpenServerSettings,
    LanguageServerPromptRequest,
    LanguageServerShowDocumentRequest,
    GitGetBranches,
    UpdateGitBranch,
    GetPathMetadata,
    CancelLanguageServerWork,
    RegisterBufferWithLanguageServers,
    GitShow,
    GitReset,
    GitDeleteBranch,
    GitCheckoutFiles,
    GitAddPathToGitignore,
    GitAddPathToGitInfoExclude,
    SetIndexText,
    ToggleLspLogs,
    GetDirectoryEnvironment,
    GetTerminalShell,

    Push,
    Fetch,
    GetRemotes,
    Pull,
    GitChangeBranch,
    GitRenameBranch,
    GitCreateBranch,
    GitCreateRemote,
    GitRemoveRemote,
    CheckForPushedCommits,
    GitDiff,
    GitInit,
    GetDocumentDiagnostics,
    PullWorkspaceDiagnostics,
    GetDefaultBranch,
    GetTreeDiff,
    GetBlobContent,
    BlameBufferAtRevision,
    LoadCommitTemplate,
    GitClone,
    GitGetWorktrees,
    GitGetHeadSha,
    GitEditRef,
    GitRepairWorktrees,
    GetCommitData,
    GetInitialGraphData,
    SearchCommits,
    GitCreateWorktree,
    GitRemoveWorktree,
    GitRenameWorktree,
    GitWorktreeCreatedAt,
    TrustWorktrees,
    RestrictWorktrees,
    FindSearchCandidatesChunk,
    FindSearchCandidatesCancelled,
    DownloadFileByPath
);

impl From<Timestamp> for SystemTime {
    fn from(val: Timestamp) -> Self {
        UNIX_EPOCH.checked_add(Duration::new(val.seconds, val.nanos)).unwrap()
    }
}

impl From<SystemTime> for Timestamp {
    fn from(time: SystemTime) -> Self {
        let duration = time.duration_since(UNIX_EPOCH).unwrap_or_default();
        Self { seconds: duration.as_secs(), nanos: duration.subsec_nanos() }
    }
}

impl From<u128> for Nonce {
    fn from(nonce: u128) -> Self {
        let upper_half = (nonce >> 64) as u64;
        let lower_half = nonce as u64;
        Self { upper_half, lower_half }
    }
}

impl From<Nonce> for u128 {
    fn from(nonce: Nonce) -> Self {
        let upper_half = (nonce.upper_half as u128) << 64;
        let lower_half = nonce.lower_half as u128;
        upper_half | lower_half
    }
}

pub const MAX_WORKTREE_UPDATE_MAX_CHUNK_SIZE: usize = 256;

pub fn split_worktree_update(mut message: UpdateWorktree) -> impl Iterator<Item = UpdateWorktree> {
    let mut done = false;

    iter::from_fn(move || {
        if done {
            return None;
        }

        let updated_entries_chunk_size = cmp::min(message.updated_entries.len(), MAX_WORKTREE_UPDATE_MAX_CHUNK_SIZE);
        let updated_entries: Vec<_> = message.updated_entries.drain(..updated_entries_chunk_size).collect();

        let removed_entries_chunk_size = cmp::min(message.removed_entries.len(), MAX_WORKTREE_UPDATE_MAX_CHUNK_SIZE);
        let removed_entries = message.removed_entries.drain(..removed_entries_chunk_size).collect();

        let mut updated_repositories = Vec::new();
        let mut limit = MAX_WORKTREE_UPDATE_MAX_CHUNK_SIZE;
        while let Some(repo) = message.updated_repositories.first_mut() {
            let updated_statuses_limit = cmp::min(repo.updated_statuses.len(), limit);
            let removed_statuses_limit = cmp::min(repo.removed_statuses.len(), limit);

            updated_repositories.push(RepositoryEntry {
                repository_id: repo.repository_id,
                branch_summary: repo.branch_summary.clone(),
                updated_statuses: repo.updated_statuses.drain(..updated_statuses_limit).collect(),
                removed_statuses: repo.removed_statuses.drain(..removed_statuses_limit).collect(),
                current_merge_conflicts: repo.current_merge_conflicts.clone(),
            });
            if repo.removed_statuses.is_empty() && repo.updated_statuses.is_empty() {
                message.updated_repositories.remove(0);
            }
            limit = limit.saturating_sub(removed_statuses_limit + updated_statuses_limit);
            if limit == 0 {
                break;
            }
        }

        done = message.updated_entries.is_empty()
            && message.removed_entries.is_empty()
            && message.updated_repositories.is_empty();

        let removed_repositories = if done { mem::take(&mut message.removed_repositories) } else { Vec::default() };

        Some(UpdateWorktree {
            project_id: message.project_id,
            worktree_id: message.worktree_id,
            root_name: message.root_name.clone(),
            abs_path: message.abs_path.clone(),
            root_repo_common_dir: message.root_repo_common_dir.clone(),
            root_repo_is_linked_worktree: message.root_repo_is_linked_worktree,
            updated_entries,
            removed_entries,
            scan_id: message.scan_id,
            is_last_update: done && message.is_last_update,
            updated_repositories,
            removed_repositories,
        })
    })
}

pub fn split_repository_update(mut update: UpdateRepository) -> impl Iterator<Item = UpdateRepository> {
    let mut updated_statuses_iter = mem::take(&mut update.updated_statuses).into_iter().fuse();
    let mut removed_statuses_iter = mem::take(&mut update.removed_statuses).into_iter().fuse();
    let branch_list = mem::take(&mut update.branch_list);
    let branch_list_error = update.branch_list_error.take();
    std::iter::from_fn({
        let update = update.clone();
        move || {
            let updated_statuses =
                updated_statuses_iter.by_ref().take(MAX_WORKTREE_UPDATE_MAX_CHUNK_SIZE).collect::<Vec<_>>();
            let removed_statuses =
                removed_statuses_iter.by_ref().take(MAX_WORKTREE_UPDATE_MAX_CHUNK_SIZE).collect::<Vec<_>>();
            if updated_statuses.is_empty() && removed_statuses.is_empty() {
                return None;
            }
            Some(UpdateRepository {
                updated_statuses,
                removed_statuses,
                branch_list: Vec::new(),
                branch_list_error: None,
                is_last_update: false,
                ..update.clone()
            })
        }
    })
    .chain([UpdateRepository {
        updated_statuses: Vec::new(),
        removed_statuses: Vec::new(),
        branch_list,
        branch_list_error,
        is_last_update: true,
        ..update
    }])
}

impl LspQuery {
    pub const fn query_name_and_write_permissions(&self) -> (&str, bool) {
        match self.request {
            Some(lsp_query::Request::GetHover(_)) => ("GetHover", false),
            Some(lsp_query::Request::GetCodeActions(_)) => ("GetCodeActions", true),
            Some(lsp_query::Request::GetCodeLens(_)) => ("GetCodeLens", true),
            Some(lsp_query::Request::GetDocumentDiagnostics(_)) => ("GetDocumentDiagnostics", false),
            Some(lsp_query::Request::GetDefinition(_)) => ("GetDefinition", false),
            Some(lsp_query::Request::GetDeclaration(_)) => ("GetDeclaration", false),
            Some(lsp_query::Request::GetTypeDefinition(_)) => ("GetTypeDefinition", false),
            Some(lsp_query::Request::GetImplementation(_)) => ("GetImplementation", false),
            Some(lsp_query::Request::GetReferences(_)) => ("GetReferences", false),
            Some(lsp_query::Request::GetDocumentColor(_)) => ("GetDocumentColor", false),
            Some(lsp_query::Request::GetFoldingRanges(_)) => ("GetFoldingRanges", false),
            Some(lsp_query::Request::GetDocumentSymbols(_)) => ("GetDocumentSymbols", false),
            Some(lsp_query::Request::GetDocumentLinks(_)) => ("GetDocumentLinks", false),
            Some(lsp_query::Request::InlayHints(_)) => ("InlayHints", false),
            Some(lsp_query::Request::SemanticTokens(_)) => ("SemanticTokens", false),
            None => ("<unknown>", true),
        }
    }
}
