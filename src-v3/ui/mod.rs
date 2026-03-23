pub mod cli_presets;
pub mod command_palette;
pub mod create_session_dialog;
pub mod deliberate_dialog;
pub mod group_grid;
pub mod settings;
pub mod sidebar;
pub mod theme;

pub use cli_presets::{CliPreset, PRESETS as CLI_PRESETS};
pub use command_palette::{
    CommandEntry, CommandPalette, CommandPaletteAction, CommandPaletteState,
    PaletteCommand,
};
pub use create_session_dialog::{CreateSessionAction, CreateSessionDialog, CreateSessionState};
pub use group_grid::{
    GroupGrid, GroupGridAction, GroupGridMember, GroupSummaryEntry,
    HybridPhase,
};
pub use sidebar::{
    GroupEntry, SessionEntry, SessionKind, SessionStatus, Sidebar,
    SidebarAction, SidebarModel,
};
pub use deliberate_dialog::{DeliberateAction, DeliberateDialog, DeliberateDialogState};
pub use settings::{SettingsAction, SettingsPanel, SettingsState};
pub use theme::{dark, light, Palette, ThemeMode};
