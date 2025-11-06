/// Centralized keybinding definitions for ChadThrottle
/// This ensures the help menu, status bar, and actual key handlers stay in sync

#[derive(Debug, Clone)]
pub struct KeyBinding {
    pub key: &'static str,
    pub description: &'static str,
    pub category: KeyCategory,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum KeyCategory {
    Navigation,
    Actions,
    System,
}

impl KeyCategory {
    pub fn title(&self) -> &'static str {
        match self {
            KeyCategory::Navigation => "Navigation",
            KeyCategory::Actions => "Actions",
            KeyCategory::System => "System",
        }
    }
}

/// Get all keybindings
pub fn get_all_keybindings() -> Vec<KeyBinding> {
    vec![
        // Navigation
        KeyBinding {
            key: "↑/k",
            description: "Move selection up",
            category: KeyCategory::Navigation,
        },
        KeyBinding {
            key: "↓/j",
            description: "Move selection down",
            category: KeyCategory::Navigation,
        },
        // Actions
        KeyBinding {
            key: "t",
            description: "Throttle selected process",
            category: KeyCategory::Actions,
        },
        KeyBinding {
            key: "r",
            description: "Remove throttle",
            category: KeyCategory::Actions,
        },
        KeyBinding {
            key: "g",
            description: "Toggle bandwidth graph",
            category: KeyCategory::Actions,
        },
        // System
        KeyBinding {
            key: "h/?",
            description: "Toggle this help",
            category: KeyCategory::System,
        },
        KeyBinding {
            key: "q/Esc/Ctrl+C",
            description: "Quit",
            category: KeyCategory::System,
        },
    ]
}

/// Get keybindings for the status bar (most common ones)
pub fn get_status_bar_keybindings() -> Vec<(&'static str, &'static str)> {
    vec![
        ("↑↓", "Navigate"),
        ("t", "Throttle"),
        ("h", "Help"),
        ("q/Ctrl+C", "Quit"),
    ]
}
