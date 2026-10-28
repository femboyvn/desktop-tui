    #[derive(Copy, Clone, PartialEq, Eq)]
    pub enum Type {
        Standard,
        Circle,
        Diamond,
        Ascii,
        Bullet,
        Target,
    }
    impl Type {
        pub(super) fn selected_symbol(&self) -> &str {
            match self {
                Type::Standard => "(●)",
                Type::Ascii => "(*)",
                Type::Circle => "⚫ ",
                Type::Diamond => "◆",
                Type::Bullet => "⦿",
                Type::Target => "🞉",
            }
        }

        pub(super) fn unselected_symbol(&self) -> &str {
            match self {
                Type::Standard => "( )",
                Type::Ascii => "( )",
                Type::Circle => "⚪ ",
                Type::Diamond => "◇",
                Type::Bullet => "⦾",
                Type::Target => "🞅",
            }
        }
    } 