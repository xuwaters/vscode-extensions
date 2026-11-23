use serde::Deserialize;
use std::collections::HashMap;

/// Everything the game reads at startup.
#[derive(Debug, Deserialize)]
pub struct GameConfig {
    /// Window size in pixels, as `(width, height)`.
    pub window_size: (u32, u32),
    pub window_title: String,
    /// Starts windowed when left out.
    #[serde(default)]
    pub fullscreen: bool,
    pub mouse_sensitivity: f32,
    /// Frame cap; `0` means uncapped.
    pub max_fps: u16,
    pub seed: u64,
    pub key_bindings: HashMap<String, Direction>,
    pub difficulty_options: DifficultyOptions,
    /// Message of the day, shown on the title screen.
    pub motd: String,
    pub icon: char,
    pub save_slot: Option<u8>,
}

#[derive(Debug, Deserialize)]
pub enum Direction {
    Up,
    Down,
    Left,
    Right,
}

#[derive(Debug, Deserialize)]
pub struct DifficultyOptions {
    pub start_difficulty: Difficulty,
    /// Raise or lower the difficulty with the player's performance.
    pub adaptive: bool,
}

#[derive(Debug, Deserialize)]
pub enum Difficulty {
    Easy,
    Normal,
    Hard,
}
