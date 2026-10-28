//! 다음 단어 고르기 — 셔플 백 (PLAN.md §5.3).
//!
//! 후보를 섞어 차례로 꺼내고, 다 꺼내면 다시 섞는다. 그래서 한 바퀴 돌 때까지 같은 단어가
//! 다시 나오지 않는다. 매번 무작위로 뽑으면 같은 단어가 연달아 나올 수 있다.

pub struct Picker {
    bag: Vec<String>,
    last: Option<String>,
    rng: fastrand::Rng,
}

impl Picker {
    pub fn new() -> Self {
        Self::with_rng(fastrand::Rng::new())
    }

    pub fn with_rng(rng: fastrand::Rng) -> Self {
        Self { bag: Vec::new(), last: None, rng }
    }

    /// 단어장을 바꿨을 때 — 다음 호출에서 새로 섞는다.
    pub fn reset(&mut self) {
        self.bag.clear();
        self.last = None;
    }

    /// `candidates`(외우지 않은 단어 키) 중 다음 단어. 후보가 없으면 `None`.
    pub fn next(&mut self, candidates: &[String]) -> Option<String> {
        if candidates.is_empty() {
            self.bag.clear();
            return None;
        }
        loop {
            match self.bag.pop() {
                // 이번 바퀴 도중에 외운 단어는 건너뛴다.
                Some(key) if candidates.contains(&key) => {
                    self.last = Some(key.clone());
                    return Some(key);
                }
                Some(_) => continue,
                None => self.refill(candidates),
            }
        }
    }

    fn refill(&mut self, candidates: &[String]) {
        let mut bag = candidates.to_vec();
        self.rng.shuffle(&mut bag);
        // pop()은 끝에서 꺼낸다. 새 바퀴의 첫 단어가 직전 단어와 같으면 맨 앞과 바꾼다.
        if bag.len() > 1 && bag.last() == self.last.as_ref() {
            let end = bag.len() - 1;
            bag.swap(0, end);
        }
        self.bag = bag;
    }
}

/// 퀴즈 문제 고르기 — 셔플 백과 달리 매번 전체에서 뽑아서 같은 단어가 또 나올 수 있다.
/// 단어마다 가중치가 0.5^streak(연속으로 맞힌 횟수)라 한 번 맞히면 다른 단어보다 반만큼 나온다.
/// `roll`은 [0, 1) 난수. 후보가 없으면 `None`.
pub fn pick_weighted(streaks: &[u32], roll: f64) -> Option<usize> {
    let weight = |streak: u32| 0.5f64.powi(streak.min(1000) as i32);
    let mut left = roll * streaks.iter().map(|&s| weight(s)).sum::<f64>();
    for (i, &s) in streaks.iter().enumerate() {
        left -= weight(s);
        if left < 0.0 {
            return Some(i);
        }
    }
    // 부동소수점 오차로 끝까지 남으면 마지막 것
    streaks.len().checked_sub(1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn weighted_pick_halves_per_correct_answer() {
        // 가중치 1 · 0.5 · 0.25 → 합 1.75 → 확률 4/7 · 2/7 · 1/7
        let mut rng = fastrand::Rng::with_seed(3);
        let mut counts = [0u32; 3];
        for _ in 0..70_000 {
            counts[pick_weighted(&[0, 1, 2], rng.f64()).unwrap()] += 1;
        }
        let share = |c: u32| c as f64 / 70_000.0;
        assert!((share(counts[0]) - 4.0 / 7.0).abs() < 0.01, "{counts:?}");
        assert!((share(counts[1]) - 2.0 / 7.0).abs() < 0.01, "{counts:?}");
        assert!((share(counts[2]) - 1.0 / 7.0).abs() < 0.01, "{counts:?}");
    }

    #[test]
    fn weighted_pick_edges() {
        assert_eq!(pick_weighted(&[], 0.5), None);
        assert_eq!(pick_weighted(&[5], 0.999), Some(0));
        assert_eq!(pick_weighted(&[0, 0], 0.0), Some(0));
        assert_eq!(pick_weighted(&[0, 0], 0.9999), Some(1));
    }

    fn words(n: usize) -> Vec<String> {
        (0..n).map(|i| format!("w{i}")).collect()
    }

    #[test]
    fn no_repeat_within_one_round() {
        let all = words(30);
        let mut picker = Picker::with_rng(fastrand::Rng::with_seed(1));
        let round: HashSet<_> = (0..30).map(|_| picker.next(&all).unwrap()).collect();
        assert_eq!(round.len(), 30);
    }

    #[test]
    fn no_immediate_repeat_across_rounds() {
        let all = words(3);
        for seed in 0..200 {
            let mut picker = Picker::with_rng(fastrand::Rng::with_seed(seed));
            let picks: Vec<_> = (0..30).map(|_| picker.next(&all).unwrap()).collect();
            assert!(picks.windows(2).all(|w| w[0] != w[1]), "seed {seed}: {picks:?}");
        }
    }

    #[test]
    fn skips_words_memorized_mid_round() {
        let all = words(10);
        let mut picker = Picker::with_rng(fastrand::Rng::with_seed(7));
        picker.next(&all).unwrap();
        let remaining: Vec<String> = all[..5].to_vec();
        for _ in 0..20 {
            let key = picker.next(&remaining).unwrap();
            assert!(remaining.contains(&key), "외운 단어 {key}가 나옴");
        }
    }

    #[test]
    fn returns_none_when_everything_is_memorized() {
        let mut picker = Picker::new();
        assert_eq!(picker.next(&[]), None);
    }

    #[test]
    fn single_candidate_keeps_coming_back() {
        let one = words(1);
        let mut picker = Picker::new();
        assert_eq!(picker.next(&one).as_deref(), Some("w0"));
        assert_eq!(picker.next(&one).as_deref(), Some("w0"));
    }
}
