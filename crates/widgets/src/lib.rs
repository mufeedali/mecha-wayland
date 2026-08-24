#![recursion_limit = "2048"]
pub mod atlas {
    include!(concat!(env!("OUT_DIR"), "/widgets_gen.rs"));
}

use renderer::commands::Color;
use taffy::prelude::*;
use taffy::{Size, Style};
use ui::widgets::{BorderColor, Div, Text};
use ui::{Damage, OnChange, Point, Render, RenderCommand, WidgetList};
use utils::Rect;

pub const BG: Color = Color::from_rgb8(24, 24, 32);
const IDLE: Color = Color::from_rgb8(46, 52, 72);
const HOT: Color = Color::from_rgb8(74, 108, 170);
const BORDER: Color = Color::from_rgb8(120, 140, 190);
const LABEL: Color = Color::from_rgb8(220, 226, 240);
const COUNT: Color = Color::from_rgb8(240, 244, 255);

const BTN: f32 = 96.0;

const FONT: &assets::BakedFont = &atlas::WIDGETS_FONT_INTER_64;

#[derive(Clone, Copy, PartialEq)]
enum Btn {
    Dec,
    Inc,
}

#[derive(Clone, Copy)]
pub struct Hover(pub Option<Point>);
#[derive(Clone, Copy)]
pub struct Click(pub Point);

pub type Button = Div<(Text,)>;

fn row_style() -> Style {
    Style {
        display: Display::Flex,
        flex_direction: FlexDirection::Row,
        justify_content: Some(JustifyContent::Center),
        align_items: Some(AlignItems::Center),
        size: Size {
            width: percent(1.0_f32),
            height: percent(1.0_f32),
        },
        gap: Size {
            width: length(28.0_f32),
            height: zero(),
        },
        ..Style::default()
    }
}

fn button_style() -> Style {
    Style {
        display: Display::Flex,
        justify_content: Some(JustifyContent::Center),
        align_items: Some(AlignItems::Center),
        size: Size {
            width: length(BTN),
            height: length(BTN),
        },
        ..Style::default()
    }
}

fn glyph(s: &str, color: Color) -> Text {
    let mut t = Text::new(Style::default());
    t.font = Some(FONT);
    t.text = s.into();
    t.color = color;
    t
}

fn button(label: &str) -> Button {
    let mut d = Div::new(button_style(), (glyph(label, LABEL),));
    d.color = IDLE;
    d.border_color = BorderColor(BORDER);
    d.border_thickness = 2.0;
    d.border_radius = 16.0;
    d
}

#[ui::widget]
pub struct Counter {
    count: u32,
    hovered: Option<Btn>,
    #[widget(child)]
    children: (Button, Text, Button),
}

impl Render for Counter {
    fn render(&self, _layout: &taffy::Layout, _abs_pos: Point) -> Vec<RenderCommand> {
        Vec::new()
    }
}

impl Counter {
    pub fn new() -> Self {
        Self {
            node_id: taffy::NodeId::new(u64::MAX),
            style: row_style(),
            bounds: Rect::ZERO,
            pending_damage: Damage::None,
            is_opaque: true,
            count: 0,
            hovered: None,
            children: (button("-"), glyph("0", COUNT), button("+")),
        }
    }

    fn hit(&self, p: Point) -> Option<Btn> {
        if self.children.0.bounds().contains_point(p) {
            Some(Btn::Dec)
        } else if self.children.2.bounds().contains_point(p) {
            Some(Btn::Inc)
        } else {
            None
        }
    }

    fn button_mut(&mut self, b: Btn) -> &mut Button {
        match b {
            Btn::Dec => &mut self.children.0,
            Btn::Inc => &mut self.children.2,
        }
    }
}

impl OnChange<Hover> for Counter {
    fn damage(&self, _new: &Hover) -> Damage {
        Damage::None
    }

    fn change(&mut self, new: Hover) {
        let target = new.0.and_then(|p| self.hit(p));
        if target == self.hovered {
            return;
        }
        if let Some(old) = self.hovered {
            self.button_mut(old).set(IDLE);
        }
        if let Some(t) = target {
            self.button_mut(t).set::<Color>(HOT);
        }
        self.hovered = target;
    }
}

impl OnChange<Click> for Counter {
    fn damage(&self, _new: &Click) -> Damage {
        Damage::None
    }

    fn change(&mut self, new: Click) {
        let Some(t) = self.hit(new.0) else {
            return;
        };
        match t {
            Btn::Inc => self.count += 1,
            Btn::Dec => self.count = self.count.saturating_sub(1),
        }
        self.children.1.set(self.count.to_string());
    }
}
