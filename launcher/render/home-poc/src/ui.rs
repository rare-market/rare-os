//! Layout shorthands in the spirit of the Tailwind classes the web screens use, in px (callers pass
//! `r * rem`). The full engine gets a typed version of the whole kit (Button, Panel, Rail, Num...).

use rare_render::taffy::prelude::*;
use rare_render::taffy::style::{AlignItems, JustifyContent, Position};

#[derive(Clone)]
pub struct S(pub Style);

pub fn s() -> S {
    // Every node is a containing block (React Native's model): absolute children sit in their parent.
    S(Style { flex_shrink: 0.0, position: Position::Relative, ..Default::default() })
}

impl S {
    pub fn row(mut self) -> S {
        self.0.flex_direction = FlexDirection::Row;
        self
    }
    pub fn col(mut self) -> S {
        self.0.flex_direction = FlexDirection::Column;
        self
    }
    pub fn w(mut self, v: f32) -> S {
        self.0.size.width = length(v);
        self
    }
    pub fn h(mut self, v: f32) -> S {
        self.0.size.height = length(v);
        self
    }
    pub fn wh(self, w: f32, h: f32) -> S {
        self.w(w).h(h)
    }
    pub fn wp(mut self, p: f32) -> S {
        self.0.size.width = percent(p);
        self
    }
    pub fn hp(mut self, p: f32) -> S {
        self.0.size.height = percent(p);
        self
    }
    pub fn min_w(mut self, v: f32) -> S {
        self.0.min_size.width = length(v);
        self
    }
    pub fn max_w(mut self, v: f32) -> S {
        self.0.max_size.width = length(v);
        self
    }
    pub fn grow(mut self) -> S {
        self.0.flex_grow = 1.0;
        self.0.flex_shrink = 1.0;
        self.0.min_size.width = length(0.0);
        self.0.min_size.height = length(0.0);
        self
    }
    pub fn gap(mut self, v: f32) -> S {
        self.0.gap = Size { width: length(v), height: length(v) };
        self
    }
    pub fn p(mut self, v: f32) -> S {
        self.0.padding = Rect { left: length(v), right: length(v), top: length(v), bottom: length(v) };
        self
    }
    pub fn px(mut self, v: f32) -> S {
        self.0.padding.left = length(v);
        self.0.padding.right = length(v);
        self
    }
    pub fn py(mut self, v: f32) -> S {
        self.0.padding.top = length(v);
        self.0.padding.bottom = length(v);
        self
    }
    pub fn pt(mut self, v: f32) -> S {
        self.0.padding.top = length(v);
        self
    }
    pub fn pb(mut self, v: f32) -> S {
        self.0.padding.bottom = length(v);
        self
    }
    pub fn mt(mut self, v: f32) -> S {
        self.0.margin.top = length(v);
        self
    }
    pub fn ml_auto(mut self) -> S {
        self.0.margin.left = auto();
        self
    }
    pub fn items_center(mut self) -> S {
        self.0.align_items = AlignItems::CENTER;
        self
    }
    pub fn items_end(mut self) -> S {
        self.0.align_items = AlignItems::FLEX_END;
        self
    }
    pub fn items_start(mut self) -> S {
        self.0.align_items = AlignItems::FLEX_START;
        self
    }
    pub fn justify_center(mut self) -> S {
        self.0.justify_content = JustifyContent::CENTER;
        self
    }
    pub fn justify_end(mut self) -> S {
        self.0.justify_content = JustifyContent::FLEX_END;
        self
    }
    pub fn justify_between(mut self) -> S {
        self.0.justify_content = JustifyContent::SPACE_BETWEEN;
        self
    }
    pub fn self_end(mut self) -> S {
        self.0.align_self = Some(AlignItems::FLEX_END);
        self
    }
    pub fn wrap(mut self) -> S {
        self.0.flex_wrap = FlexWrap::Wrap;
        self
    }
    /// position: absolute with the given insets (None = auto).
    pub fn abs(mut self, top: Option<f32>, right: Option<f32>, bottom: Option<f32>, left: Option<f32>) -> S {
        self.0.position = Position::Absolute;
        let v = |x: Option<f32>| x.map(length).unwrap_or(auto());
        self.0.inset = Rect { top: v(top), right: v(right), bottom: v(bottom), left: v(left) };
        self
    }
    pub fn fill(self) -> S {
        self.abs(Some(0.0), Some(0.0), Some(0.0), Some(0.0))
    }
    pub fn done(self) -> Style {
        self.0
    }
}

impl From<S> for Style {
    fn from(s: S) -> Style {
        s.0
    }
}
