use super::{Phase, Random};

pub(super) const SIDE: usize = 9;
pub(super) const MINES: usize = 10;
#[derive(Clone, Copy, Default)]
pub(super) struct Cell { pub mine: bool, pub revealed: bool, pub flagged: bool }
pub(super) struct Minesweeper {
    pub cells: [Cell; SIDE * SIDE],
    pub phase: Phase,
    planted: bool,
    random: Random,
}
impl Minesweeper {
    pub fn new(seed: u64) -> Self {
        Self { cells: [Cell::default(); SIDE * SIDE], phase: Phase::Ready, planted: false, random: Random::new(seed) }
    }
    pub fn reveal(&mut self, index: usize) {
        if index >= self.cells.len() || matches!(self.phase, Phase::Lost | Phase::Won)
            || self.cells[index].flagged || self.cells[index].revealed { return; }
        if !self.planted {
            let mut candidates: Vec<_> = (0..self.cells.len()).filter(|&other| {
                (index % SIDE).abs_diff(other % SIDE) > 1 || (index / SIDE).abs_diff(other / SIDE) > 1
            }).collect();
            for first in 0..MINES {
                let selected = first + self.random.below(candidates.len() - first);
                candidates.swap(first, selected);
                self.cells[candidates[first]].mine = true;
            }
            self.planted = true;
        }
        self.phase = Phase::Playing;
        if self.cells[index].mine { self.cells[index].revealed = true; self.phase = Phase::Lost; return; }
        let mut pending = vec![index];
        while let Some(index) = pending.pop() {
            let cell = &mut self.cells[index];
            if cell.revealed || cell.flagged || cell.mine { continue; }
            cell.revealed = true;
            if self.adjacent(index) == 0 { pending.extend(Self::neighbors(index)); }
        }
        if self.cells.iter().all(|cell| cell.mine || cell.revealed) { self.phase = Phase::Won; }
    }
    pub fn flag(&mut self, index: usize) {
        if matches!(self.phase, Phase::Lost | Phase::Won) { return; }
        if let Some(cell) = self.cells.get_mut(index)
            && !cell.revealed { cell.flagged = !cell.flagged; }
    }
    pub fn adjacent(&self, index: usize) -> usize {
        Self::neighbors(index).filter(|&other| self.cells[other].mine).count()
    }
    fn neighbors(index: usize) -> impl Iterator<Item = usize> {
        let (x, y) = (index % SIDE, index / SIDE);
        let left = x.saturating_sub(1);
        let top = y.saturating_sub(1);
        (top..=(y + 1).min(SIDE - 1)).flat_map(move |row| {
            (left..=(x + 1).min(SIDE - 1)).map(move |column| row * SIDE + column)
        }).filter(move |&other| other != index)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn first_reveal_has_a_safe_neighborhood_and_exactly_ten_unique_mines() {
        for seed in 0..100 {
            for first in [0, 40, 80] {
                let mut game = Minesweeper::new(seed);
                game.reveal(first);
                assert_eq!(game.cells.iter().filter(|cell| cell.mine).count(), 10);
                assert!(game.cells[first].revealed);
                assert!(!game.cells[first].mine);
                assert_eq!(game.adjacent(first), 0);
                assert_eq!(game.phase, Phase::Playing);
            }
        }
    }
    #[test]
    fn flags_block_reveal_and_flood_fill_until_removed() {
        let mut game = Minesweeper::new(1);
        game.planted = true;
        game.cells[0].mine = true;
        game.flag(80);
        game.reveal(80);
        assert!(!game.cells[80].revealed);
        game.reveal(79);
        assert!(!game.cells[80].revealed);
        assert_eq!(game.adjacent(1), 1);
        assert_eq!(game.adjacent(10), 1);
        assert_eq!(game.adjacent(2), 0);
        assert!(game.cells[1].revealed);
        assert!(!game.cells[0].revealed);
        game.flag(80);
        game.reveal(80);
        assert_eq!(game.phase, Phase::Won);
    }
    #[test]
    fn mine_reveal_loses_and_finished_boards_ignore_actions() {
        let mut game = Minesweeper::new(1);
        game.planted = true;
        game.cells[40].mine = true;
        game.reveal(40);
        assert_eq!(game.phase, Phase::Lost);
        assert!(game.cells[40].revealed);
        game.flag(0); game.reveal(0);
        assert!(!game.cells[0].flagged && !game.cells[0].revealed);
        game.reveal(81); game.flag(usize::MAX);
    }
}
