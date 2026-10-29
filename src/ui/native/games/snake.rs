use super::{Direction, Phase, Random};
use std::collections::VecDeque;

pub(super) const WIDTH: i32 = 20;
pub(super) const HEIGHT: i32 = 12;
pub(super) struct Snake {
    pub body: VecDeque<(i32, i32)>,
    pub food: Option<(i32, i32)>,
    pub phase: Phase,
    pub score: u32,
    direction: Direction,
    pending: Option<Direction>,
    random: Random,
}
impl Snake {
    pub fn new(seed: u64) -> Self {
        let mut game = Self { body: VecDeque::from([(6, 6), (5, 6), (4, 6)]), food: None, phase: Phase::Ready,
            score: 0, direction: Direction::Right, pending: None, random: Random::new(seed) };
        game.place_food();
        game
    }
    pub fn turn(&mut self, direction: Direction) {
        let (x, y) = self.direction.delta();
        if self.pending.is_none() && direction.delta() != (-x, -y) { self.pending = Some(direction); }
    }
    pub fn step(&mut self) {
        if self.phase != Phase::Playing { return; }
        if let Some(direction) = self.pending.take() { self.direction = direction; }
        let (x, y) = self.body[0];
        let (dx, dy) = self.direction.delta();
        let next = (x + dx, y + dy);
        let eating = self.food == Some(next);
        // The tail leaves on this tick unless food is being eaten.
        let occupied = self.body.len() - usize::from(!eating);
        if !(0..WIDTH).contains(&next.0) || !(0..HEIGHT).contains(&next.1)
            || self.body.iter().take(occupied).any(|&cell| cell == next) {
            self.phase = Phase::Lost;
            return;
        }
        self.body.push_front(next);
        if eating { self.score += 10; self.place_food(); }
        else { self.body.pop_back(); }
    }
    fn place_food(&mut self) {
        let free: Vec<_> = (0..HEIGHT).flat_map(|y| (0..WIDTH).map(move |x| (x, y)))
            .filter(|cell| !self.body.contains(cell)).collect();
        if free.is_empty() { self.food = None; self.phase = Phase::Won; }
        else { self.food = Some(free[self.random.below(free.len())]); }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn eating_grows_scores_and_places_food_off_the_body() {
        let mut game = Snake::new(1);
        game.food = Some((7, 6));
        game.phase = Phase::Playing;
        game.step();
        assert_eq!(game.body.front(), Some(&(7, 6)));
        assert_eq!(game.body.len(), 4);
        assert_eq!(game.score, 10);
        assert!(!game.body.contains(&game.food.unwrap()));
    }
    #[test]
    fn rapid_turns_cannot_reverse_and_inactive_games_do_not_move() {
        let mut game = Snake::new(1);
        game.phase = Phase::Playing;
        game.turn(Direction::Up);
        game.turn(Direction::Left);
        game.step();
        assert_eq!(game.body.front(), Some(&(6, 5)));
        game.turn(Direction::Down);
        game.step();
        assert_eq!(game.body.front(), Some(&(6, 4)));
        for phase in [Phase::Ready, Phase::Paused, Phase::Lost, Phase::Won] {
            game.phase = phase;
            let body = game.body.clone();
            game.step();
            assert_eq!(game.body, body);
        }
    }
    #[test]
    fn wall_and_body_collisions_end_the_round_but_vacated_tail_is_legal() {
        let mut game = Snake::new(1);
        game.phase = Phase::Playing;
        game.body = VecDeque::from([(19, 0), (18, 0), (17, 0)]);
        game.step();
        assert_eq!(game.phase, Phase::Lost);
        game.phase = Phase::Playing;
        game.body = VecDeque::from([(2, 2), (2, 3), (3, 3), (3, 2)]);
        game.step();
        assert_eq!(game.phase, Phase::Playing);
        assert_eq!(game.body.front(), Some(&(3, 2)));
        game.body = VecDeque::from([(2, 2), (2, 3), (3, 3), (3, 2), (4, 2)]);
        game.step();
        assert_eq!(game.phase, Phase::Lost);
    }
    #[test]
    fn filling_the_board_wins_without_searching_forever_for_food() {
        let mut game = Snake::new(1);
        game.body = VecDeque::from([(0, 0)]);
        for y in 0..HEIGHT { for x in 0..WIDTH {
            if (x, y) != (0, 0) && (x, y) != (1, 0) { game.body.push_back((x, y)); }
        } }
        game.food = Some((1, 0));
        game.phase = Phase::Playing;
        game.step();
        assert_eq!(game.phase, Phase::Won);
        assert_eq!(game.food, None);
        assert_eq!(game.body.len(), 240);
    }
}
