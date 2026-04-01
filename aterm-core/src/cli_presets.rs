pub struct CliPreset {
    pub id: &'static str,
    pub label: &'static str,
    pub command: &'static str,
    pub args: &'static [&'static str],
    pub icon: &'static str,
}

pub const PRESETS: &[CliPreset] = &[
    CliPreset {
        id: "claude",
        label: "Claude",
        command: "claude",
        args: &["--dangerously-skip-permissions", "--continue"],
        icon: "\u{2733}",
    },
    CliPreset {
        id: "codex",
        label: "Codex",
        command: "codex",
        args: &[
            "resume",
            "--last",
            "--dangerously-bypass-approvals-and-sandbox",
        ],
        icon: "\u{2318}",
    },
    CliPreset {
        id: "gemini",
        label: "Gemini",
        command: "gemini",
        args: &["resume", "-y"],
        icon: "\u{2726}",
    },
    CliPreset {
        id: "custom",
        label: "Custom",
        command: "",
        args: &[],
        icon: "\u{2699}",
    },
];

pub fn preset_by_id(id: &str) -> Option<&'static CliPreset> {
    PRESETS.iter().find(|p| p.id == id)
}
