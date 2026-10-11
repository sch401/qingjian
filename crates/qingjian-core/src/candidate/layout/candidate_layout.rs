//! 云候选分页布局；可显式选择 AI 候选优先，自定义短语保留固定位置。

use super::super::{Candidate, CandidateKind};
use super::Cell;

/// 本地候选 + 云端词的分页排布；旧入口放在末尾，优先入口按模型顺序置顶。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CandidateLayout {
    /// 本地候选，顺序就是 Engine 排好的顺序。
    local: Vec<Candidate>,

    /// 已到的云端词；同文的普通本地候选在显示时隐藏。
    cloud: Vec<Candidate>,

    /// 每页几格。
    page_size: usize,

    /// 配置的云端槽位数；0 表示不要云端词。
    slots: usize,

    /// 当前云候选是否采用优先入口；旧平台调用保持原末尾布局。
    cloud_first: bool,
}

impl CandidateLayout {
    pub fn new(local: Vec<Candidate>, page_size: usize, slots: usize) -> Self {
        Self {
            local,
            cloud: Vec::new(),
            page_size: page_size.max(1),
            slots,
            cloud_first: false,
        }
    }

    pub fn page_size(&self) -> usize {
        self.page_size
    }

    pub fn local(&self) -> &[Candidate] {
        &self.local
    }

    pub fn cloud(&self) -> &[Candidate] {
        &self.cloud
    }

    /// 第一页最多给云端几格，避开自定义短语的固定位置；问字模式整页都给云端。
    pub fn capacity(&self) -> usize {
        if self.local.is_empty() {
            self.page_size
        } else if !self.cloud_first {
            let fixed = self
                .local
                .iter()
                .filter_map(|c| match c.kind {
                    CandidateKind::Custom(n) => Some(n),
                    _ => None,
                })
                .max()
                .unwrap_or(1);
            self.slots.min(self.page_size.saturating_sub(fixed.max(1)))
        } else {
            let fixed = self
                .local
                .iter()
                .filter(
                    |c| matches!(c.kind, CandidateKind::Custom(n) if n > 0 && n <= self.page_size),
                )
                .count();
            self.slots.min(self.page_size.saturating_sub(fixed))
        }
    }

    /// 保留旧平台入口：过滤本地同文候选，补入第一页末尾。
    pub fn set_cloud(&mut self, words: Vec<Candidate>) -> usize {
        self.replace_cloud(words, false)
    }

    /// 按模型顺序置顶；同文候选合并，自定义短语固定位置优先。返回采纳的条数。
    pub fn set_cloud_first(&mut self, words: Vec<Candidate>) -> usize {
        self.replace_cloud(words, true)
    }

    fn replace_cloud(&mut self, words: Vec<Candidate>, first: bool) -> usize {
        self.cloud_first = first;
        self.cloud.clear();
        let capacity = self.capacity();
        for mut word in words {
            if self.cloud.len() >= capacity
                || self.local.iter().any(|c| {
                    c.text == word.text && (!first || matches!(c.kind, CandidateKind::Custom(_)))
                })
                || self.cloud.iter().any(|c| c.text == word.text)
            {
                continue;
            }
            word.kind = CandidateKind::Cloud;
            self.cloud.push(word);
        }
        self.cloud.len()
    }

    fn visible_local(&self) -> impl Iterator<Item = &Candidate> {
        self.local
            .iter()
            .filter(|c| !self.cloud.iter().any(|word| word.text == c.text))
    }

    /// 固定位置先放好，普通候选按当前入口的顺序填入。
    pub fn cells(&self) -> Vec<Cell<'_>> {
        let local_len = self.len()
            - if self.cloud_first {
                0
            } else {
                self.cloud.len()
            };
        let mut cells = vec![Cell::Empty; local_len];
        for candidate in &self.local {
            if let CandidateKind::Custom(position) = candidate.kind
                && let Some(cell) = position.checked_sub(1).and_then(|i| cells.get_mut(i))
            {
                *cell = Cell::Local(candidate);
            }
        }
        let mut normal = self
            .cloud
            .iter()
            .filter(|_| self.cloud_first)
            .map(Cell::Cloud)
            .chain(
                self.visible_local()
                    .filter(|c| !matches!(c.kind, CandidateKind::Custom(_)))
                    .map(Cell::Local),
            );
        for cell in &mut cells {
            if matches!(cell, Cell::Empty)
                && let Some(candidate) = normal.next()
            {
                *cell = candidate;
            }
        }
        if !self.cloud_first {
            let first = cells.len().min(self.page_size - self.cloud.len());
            cells.splice(first..first, self.cloud.iter().map(Cell::Cloud));
        }
        cells
    }

    pub fn len(&self) -> usize {
        let fixed = self
            .local
            .iter()
            .filter_map(|c| match c.kind {
                CandidateKind::Custom(position) => Some(position),
                _ => None,
            })
            .max()
            .unwrap_or(0);
        if self.cloud_first {
            fixed.max(self.visible_local().count() + self.cloud.len())
        } else {
            fixed.max(self.local.len()) + self.cloud.len()
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn pages(&self) -> usize {
        self.len().div_ceil(self.page_size)
    }

    /// 第 `index` 格的候选；越界返回 `None`。
    pub fn candidate(&self, index: usize) -> Option<&Candidate> {
        self.cells().get(index).and_then(|cell| cell.candidate())
    }

    /// 第 `page` 页的格子。
    pub fn page(&self, page: usize) -> Vec<Cell<'_>> {
        self.cells()
            .into_iter()
            .skip(page * self.page_size)
            .take(self.page_size)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn local(text: &str) -> Candidate {
        Candidate {
            text: text.into(),
            kind: CandidateKind::Chinese,
            syllables: vec!["zhang".into(), "tao".into()],
            reading: None,
            translation: None,
            aux_code: None,
        }
    }

    fn cloud(text: &str) -> Candidate {
        Candidate {
            kind: CandidateKind::Cloud,
            ..local(text)
        }
    }

    fn texts(cells: &[Cell<'_>]) -> Vec<String> {
        cells
            .iter()
            .map(|cell| match cell {
                Cell::Local(c) => c.text.clone(),
                Cell::Cloud(c) => format!("☁{}", c.text),
                Cell::Empty => "<empty>".into(),
            })
            .collect()
    }

    fn many(count: usize) -> Vec<Candidate> {
        (0..count).map(|i| local(&format!("本{i}"))).collect()
    }

    #[test]
    fn cloud_words_take_the_front_and_local_candidates_keep_their_order() {
        let mut layout = CandidateLayout::new(many(12), 9, 3);
        assert_eq!(
            texts(&layout.page(0)),
            [
                "本0", "本1", "本2", "本3", "本4", "本5", "本6", "本7", "本8"
            ]
        );
        assert_eq!(layout.pages(), 2);

        // 四条云端词只取前三条，原本的本地候选按原顺序接在后面。
        let filled =
            layout.set_cloud_first(vec![cloud("云0"), cloud("云1"), cloud("云2"), cloud("云3")]);
        assert_eq!(filled, 3);
        assert_eq!(
            texts(&layout.page(0)),
            [
                "☁云0", "☁云1", "☁云2", "本0", "本1", "本2", "本3", "本4", "本5"
            ]
        );
        assert_eq!(
            texts(&layout.page(1)),
            ["本6", "本7", "本8", "本9", "本10", "本11"]
        );
        assert_eq!(layout.candidate(0).unwrap().text, "云0");
        assert_eq!(layout.candidate(9).unwrap().text, "本6");
    }

    #[test]
    fn local_duplicates_are_promoted_and_restored_when_cloud_is_cleared() {
        let mut layout = CandidateLayout::new(many(4), 9, 2);
        assert_eq!(
            layout.set_cloud_first(vec![cloud("本2"), cloud("本2"), cloud("云0")]),
            2
        );
        assert_eq!(
            texts(&layout.page(0)),
            ["☁本2", "☁云0", "本0", "本1", "本3"]
        );
        assert_eq!(layout.len(), 5);
        assert_eq!(layout.local()[2].kind, CandidateKind::Chinese);
        assert_eq!(layout.pages(), 1);
        layout.set_cloud_first(Vec::new());
        assert_eq!(texts(&layout.page(0)), ["本0", "本1", "本2", "本3"]);
    }

    #[test]
    fn no_words_means_nothing_changes() {
        let mut layout = CandidateLayout::new(many(12), 9, 2);
        assert_eq!(layout.set_cloud_first(Vec::new()), 0);
        assert_eq!(layout.page(0).len(), 9);
        assert_eq!(texts(&layout.page(1)), ["本9", "本10", "本11"]);
    }

    #[test]
    fn zero_slots_means_no_cloud_cells_at_all() {
        let mut layout = CandidateLayout::new(many(3), 9, 0);
        assert_eq!(layout.set_cloud_first(vec![cloud("云0")]), 0);
        assert_eq!(texts(&layout.page(0)), ["本0", "本1", "本2"]);
    }

    #[test]
    fn without_local_candidates_the_whole_page_goes_to_the_cloud() {
        let mut layout = CandidateLayout::new(Vec::new(), 9, 2);
        assert_eq!(
            layout.set_cloud_first(vec![cloud("森"), cloud("淼"), cloud("焱")]),
            3
        );
        assert_eq!(texts(&layout.page(0)), ["☁森", "☁淼", "☁焱"]);
        assert_eq!(layout.candidate(0).unwrap().text, "森");
    }

    #[test]
    fn cloud_can_fill_the_first_page_and_local_candidates_follow() {
        let mut layout = CandidateLayout::new(many(5), 3, 5);
        layout.set_cloud_first(vec![cloud("云0"), cloud("云1"), cloud("云2")]);
        assert_eq!(layout.capacity(), 3);
        assert_eq!(texts(&layout.page(0)), ["☁云0", "☁云1", "☁云2"]);
        assert_eq!(texts(&layout.page(1)), ["本0", "本1", "本2"]);
        let mut single = CandidateLayout::new(many(1), 1, 1);
        assert_eq!(single.set_cloud_first(vec![cloud("云")]), 1);
        assert_eq!(texts(&single.page(0)), ["☁云"]);
    }

    #[test]
    fn sparse_custom_positions_are_layout_cells_not_candidates() {
        let fixed = Candidate {
            kind: CandidateKind::Custom(3),
            ..local("短语")
        };
        let mut layout = CandidateLayout::new(vec![fixed, local("普通")], 5, 2);
        assert_eq!(layout.local().len(), 2);
        assert_eq!(texts(&layout.page(0)), ["普通", "<empty>", "短语"]);
        assert!(layout.candidate(1).is_none());
        assert_eq!(layout.set_cloud_first(vec![cloud("云")]), 1);
        assert_eq!(texts(&layout.page(0)), ["☁云", "普通", "短语"]);
        assert_eq!(layout.set_cloud_first(vec![cloud("短语"), cloud("云")]), 1);
        assert_eq!(layout.candidate(2).unwrap().text, "短语");
    }

    #[test]
    fn ninth_position_stays_on_second_page_without_cloud_displacement() {
        let mut layout = CandidateLayout::new(
            vec![Candidate {
                kind: CandidateKind::Custom(9),
                ..local("第九")
            }],
            5,
            2,
        );
        assert_eq!(layout.pages(), 2);
        assert_eq!(layout.capacity(), 0);
        assert_eq!(layout.set_cloud_first(vec![cloud("云")]), 1);
        assert_eq!(layout.capacity(), 2);
        assert_eq!(layout.candidate(8).unwrap().text, "第九");
        assert!(layout.candidate(7).is_none());
    }

    #[test]
    fn legacy_entry_keeps_tail_slots_and_drops_local_duplicates() {
        let mut layout = CandidateLayout::new(many(4), 9, 2);
        layout.set_cloud_first(vec![cloud("本2"), cloud("云")]);
        assert_eq!(layout.set_cloud(vec![cloud("本2"), cloud("云")]), 1);
        assert_eq!(texts(&layout.page(0)), ["本0", "本1", "本2", "本3", "☁云"]);
    }

    #[test]
    fn legacy_entry_reserves_local_first_and_sparse_fixed_positions() {
        let mut single = CandidateLayout::new(many(1), 1, 1);
        assert_eq!(single.set_cloud(vec![cloud("云")]), 0);
        let mut fixed = CandidateLayout::new(
            vec![Candidate {
                kind: CandidateKind::Custom(9),
                ..local("第九")
            }],
            5,
            2,
        );
        assert_eq!(fixed.set_cloud(vec![cloud("云")]), 0);
        assert_eq!(fixed.candidate(8).unwrap().text, "第九");
    }
}
