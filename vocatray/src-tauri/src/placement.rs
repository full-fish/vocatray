//! 카드 창을 트레이 아이콘 옆에 놓는 위치 계산 (모두 물리 픽셀).

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Area {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Area {
    /// 점이 영역 안에 있는가 (오른쪽·아래 끝은 바깥)
    pub fn contains(&self, x: f64, y: f64) -> bool {
        (self.x..self.x + self.w).contains(&x) && (self.y..self.y + self.h).contains(&y)
    }
}

/// 아이콘 위에 자리가 있으면 위(Windows 작업 표시줄), 없으면 아래(macOS 메뉴 막대)에 놓고,
/// 화면 가장자리를 넘지 않게 작업 영역(`work`, 작업 표시줄 제외) 안으로 밀어 넣는다.
pub fn place_card(icon: Area, card_w: f64, card_h: f64, work: Area, gap: f64) -> (f64, f64) {
    let centered = icon.x + icon.w / 2.0 - card_w / 2.0;
    let x = clamp(centered, work.x + gap, work.x + work.w - card_w - gap);
    let above = icon.y - gap - card_h;
    let y = if above >= work.y { above } else { icon.y + icon.h + gap };
    let y = clamp(y, work.y, work.y + work.h - card_h);
    (x, y)
}

/// 카드가 작업 영역보다 커서 min > max가 돼도 패닉하지 않는 clamp (min 우선).
fn clamp(v: f64, min: f64, max: f64) -> f64 {
    v.min(max).max(min)
}

#[cfg(test)]
mod tests {
    use super::*;

    // 1920x1080 화면, 아래쪽 48px 작업 표시줄
    const WORK: Area = Area { x: 0.0, y: 0.0, w: 1920.0, h: 1032.0 };

    #[test]
    fn contains_excludes_right_and_bottom_edges() {
        let icon = Area { x: 10.0, y: 20.0, w: 30.0, h: 30.0 };
        assert!(icon.contains(10.0, 20.0));
        assert!(icon.contains(39.5, 49.5));
        assert!(!icon.contains(40.0, 30.0));
        assert!(!icon.contains(20.0, 50.0));
        assert!(!icon.contains(9.5, 30.0));
    }

    #[test]
    fn bottom_taskbar_puts_card_above_icon() {
        let icon = Area { x: 1500.0, y: 1040.0, w: 32.0, h: 32.0 };
        let (x, y) = place_card(icon, 340.0, 200.0, WORK, 8.0);
        assert_eq!(x, 1516.0 - 170.0);
        assert_eq!(y, 1040.0 - 8.0 - 200.0);
    }

    #[test]
    fn icon_near_right_edge_keeps_card_on_screen() {
        let icon = Area { x: 1880.0, y: 1040.0, w: 32.0, h: 32.0 };
        let (x, _) = place_card(icon, 340.0, 200.0, WORK, 8.0);
        assert_eq!(x, 1920.0 - 340.0 - 8.0);
    }

    #[test]
    fn top_menu_bar_puts_card_below_icon() {
        // macOS: 메뉴 막대(0~25px)는 작업 영역 밖
        let work = Area { x: 0.0, y: 25.0, w: 1440.0, h: 875.0 };
        let icon = Area { x: 1200.0, y: 0.0, w: 24.0, h: 24.0 };
        let (_, y) = place_card(icon, 340.0, 200.0, work, 8.0);
        assert_eq!(y, 24.0 + 8.0);
    }

    #[test]
    fn card_bigger_than_work_area_does_not_panic() {
        let tiny = Area { x: 0.0, y: 0.0, w: 100.0, h: 100.0 };
        let icon = Area { x: 50.0, y: 90.0, w: 10.0, h: 10.0 };
        let (x, y) = place_card(icon, 340.0, 200.0, tiny, 8.0);
        assert!(x.is_finite() && y.is_finite());
    }
}
