use super::{Direction, Phase, Random};

pub(super) struct Puzzle2048 {
    pub board: [u32; 16],
    pub score: u64,
    pub phase: Phase,
    random: Random,
}
impl Puzzle2048 {
    pub fn new(seed: u64) -> Self {
        let mut game = Self { board: [0; 16], score: 0, phase: Phase::Playing, random: Random::new(seed) };
        game.spawn(); game.spawn();
        game
    }
    pub fn slide(&mut self, direction: Direction) {
        if self.phase != Phase::Playing { return; }
        let before = self.board;
        for row in 0..4 {
            let indices: [usize; 4] = std::array::from_fn(|column| match direction {
                Direction::Left => row * 4 + column,
                Direction::Right => row * 4 + 3 - column,
                Direction::Up => column * 4 + row,
                Direction::Down => (3 - column) * 4 + row,
            });
            let (merged, score) = Self::merge(indices.map(|index| self.board[index]));
            for (index, value) in indices.into_iter().zip(merged) { self.board[index] = value; }
            self.score += score;
        }
        if self.board != before { self.spawn(); }
        if self.board.iter().any(|&value| value >= 2048) { self.phase = Phase::Won; }
        else if !self.board.contains(&0) && !(0..16).any(|index| {
            (index % 4 < 3 && self.board[index] == self.board[index + 1])
                || (index / 4 < 3 && self.board[index] == self.board[index + 4])
        }) { self.phase = Phase::Lost; }
    }
    fn merge(line: [u32; 4]) -> ([u32; 4], u64) {
        let tiles: Vec<_> = line.into_iter().filter(|&value| value != 0).collect();
        let mut result = [0; 4];
        let (mut source, mut target, mut score) = (0, 0, 0);
        while source < tiles.len() {
            let value = tiles[source];
            if source + 1 < tiles.len() && value == tiles[source + 1] {
                result[target] = value * 2;
                score += u64::from(value * 2);
                source += 2;
            } else { result[target] = value; source += 1; }
            target += 1;
        }
        (result, score)
    }
    fn spawn(&mut self) {
        let free: Vec<_> = (0..16).filter(|&index| self.board[index] == 0).collect();
        if !free.is_empty() {
            let index = free[self.random.below(free.len())];
            self.board[index] = if self.random.below(10) == 0 { 4 } else { 2 };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn adjacent_tiles_merge_once_after_compression() {
        for (line, expected, score) in [
            ([2, 2, 2, 2], [4, 4, 0, 0], 8),
            ([2, 0, 2, 4], [4, 4, 0, 0], 4),
            ([4, 4, 8, 0], [8, 8, 0, 0], 8),
            ([0, 0, 0, 2], [2, 0, 0, 0], 0),
        ] { assert_eq!(Puzzle2048::merge(line), (expected, score)); }
    }
    #[test]
    fn starts_with_two_tiles_and_only_changed_moves_spawn() {
        let mut game = Puzzle2048::new(7);
        assert_eq!(game.board.iter().filter(|&&value| value != 0).count(), 2);
        game.board = [2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        game.slide(Direction::Left);
        assert_eq!(game.board, [2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        for (direction, expected) in [(Direction::Right, 7), (Direction::Down, 13), (Direction::Up, 1), (Direction::Left, 4)] {
            game.board = [0; 16]; game.board[5] = 8;
            game.slide(direction);
            assert_eq!(game.board[expected], 8, "{direction:?}");
            assert_eq!(game.board.iter().filter(|&&value| value != 0).count(), 2);
            assert!(game.board.iter().filter(|&&value| value != 8 && value != 0).all(|&value| value == 2 || value == 4));
        }
    }
    #[test]
    fn reaching_2048_wins_and_only_an_unmergeable_full_board_loses() {
        let mut game = Puzzle2048::new(1);
        game.board = [1024, 1024, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        game.slide(Direction::Left);
        assert_eq!(game.phase, Phase::Won);
        assert_eq!(game.score, 2048);
        game.phase = Phase::Playing;
        game.board = [2,4,2,4, 4,2,4,2, 2,4,2,4, 4,2,4,2];
        game.slide(Direction::Up);
        assert_eq!(game.phase, Phase::Lost);
        let board = game.board;
        game.slide(Direction::Right);
        assert_eq!(game.board, board);
        game.phase = Phase::Playing;
        game.board[0] = 4;
        game.board[15] = 0;
        game.slide(Direction::Right);
        assert_eq!(game.phase, Phase::Playing);
        assert_eq!(game.score, 2056);
    }
}
