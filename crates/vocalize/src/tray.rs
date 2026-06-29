//! System-tray menu for vocalize: toggle, tonality, scale, mode, chord playback,
//! timbre, tolerance, sustain, sound, and tone replay.

use futures::channel::mpsc::UnboundedSender;

use crate::exercise::{Mode, PlayStyle, ScaleKind};
use crate::tone::Timbre;
use crate::{CENTS_STEPS, Message, ROOTS, SUSTAIN_STEPS, exercise};

pub struct VocalizeTray {
    pub tx: UnboundedSender<Message>,
    pub enabled: bool,
    pub audible: bool,
    pub scale_root: i64,
    pub scale_kind: ScaleKind,
    pub mode: Mode,
    pub play_style: PlayStyle,
    pub timbre: Timbre,
    pub cents_window: f64,
    pub sustain_ms: f64,
}

impl ksni::Tray for VocalizeTray {
    fn icon_name(&self) -> String {
        "audio-input-microphone".into()
    }
    fn title(&self) -> String {
        "sceno · vocalize".into()
    }
    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        use ksni::menu::*;
        let root_idx = ROOTS
            .iter()
            .position(|&r| r == self.scale_root)
            .unwrap_or(0);
        let kind_idx = self.scale_kind.index();
        let mode_idx = self.mode.index();
        let play_style_idx = self.play_style.index();
        let timbre_idx = self.timbre.index();
        let cents_idx = CENTS_STEPS
            .iter()
            .position(|&c| (c - self.cents_window).abs() < 0.5)
            .unwrap_or(1);
        let sustain_idx = SUSTAIN_STEPS
            .iter()
            .position(|&s| (s - self.sustain_ms).abs() < 0.5)
            .unwrap_or(1);
        vec![
            CheckmarkItem {
                label: "Overlay ativo".into(),
                checked: self.enabled,
                activate: Box::new(|this: &mut Self| {
                    this.enabled = !this.enabled;
                    let _ = this.tx.unbounded_send(Message::SetEnabled(this.enabled));
                }),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            SubMenu {
                label: "Tonalidade".into(),
                submenu: vec![
                    RadioGroup {
                        selected: root_idx,
                        select: Box::new(|this: &mut Self, idx| {
                            let r = ROOTS.get(idx).copied().unwrap_or(0);
                            this.scale_root = r;
                            let _ = this.tx.unbounded_send(Message::SetRoot(r));
                        }),
                        options: ROOTS
                            .iter()
                            .map(|&r| RadioItem {
                                // Root label in the playback octave, e.g. "Dó (C)".
                                label: exercise::note_label(60 + r),
                                ..Default::default()
                            })
                            .collect(),
                    }
                    .into(),
                ],
                ..Default::default()
            }
            .into(),
            SubMenu {
                label: "Escala".into(),
                submenu: vec![
                    RadioGroup {
                        selected: kind_idx,
                        select: Box::new(|this: &mut Self, idx| {
                            let k = ScaleKind::from_idx(idx);
                            this.scale_kind = k;
                            let _ = this.tx.unbounded_send(Message::SetScaleKind(k));
                        }),
                        options: ScaleKind::ALL
                            .iter()
                            .map(|k| RadioItem {
                                label: k.label().into(),
                                ..Default::default()
                            })
                            .collect(),
                    }
                    .into(),
                ],
                ..Default::default()
            }
            .into(),
            SubMenu {
                label: "Modo".into(),
                submenu: vec![
                    RadioGroup {
                        selected: mode_idx,
                        select: Box::new(|this: &mut Self, idx| {
                            let m = Mode::from_idx(idx);
                            this.mode = m;
                            let _ = this.tx.unbounded_send(Message::SetMode(m));
                        }),
                        options: Mode::ALL
                            .iter()
                            .map(|m| RadioItem {
                                label: m.label().into(),
                                ..Default::default()
                            })
                            .collect(),
                    }
                    .into(),
                ],
                ..Default::default()
            }
            .into(),
            SubMenu {
                label: "Reprodução".into(),
                submenu: vec![
                    RadioGroup {
                        selected: play_style_idx,
                        select: Box::new(|this: &mut Self, idx| {
                            let s = PlayStyle::from_idx(idx);
                            this.play_style = s;
                            let _ = this.tx.unbounded_send(Message::SetPlayStyle(s));
                        }),
                        options: PlayStyle::ALL
                            .iter()
                            .map(|s| RadioItem {
                                label: s.label().into(),
                                ..Default::default()
                            })
                            .collect(),
                    }
                    .into(),
                ],
                ..Default::default()
            }
            .into(),
            SubMenu {
                label: "Timbre".into(),
                submenu: vec![
                    RadioGroup {
                        selected: timbre_idx,
                        select: Box::new(|this: &mut Self, idx| {
                            let t = Timbre::from_idx(idx);
                            this.timbre = t;
                            let _ = this.tx.unbounded_send(Message::SetTimbre(t));
                        }),
                        options: Timbre::ALL
                            .iter()
                            .map(|t| RadioItem {
                                label: t.label().into(),
                                ..Default::default()
                            })
                            .collect(),
                    }
                    .into(),
                ],
                ..Default::default()
            }
            .into(),
            SubMenu {
                label: "Tolerância".into(),
                submenu: vec![
                    RadioGroup {
                        selected: cents_idx,
                        select: Box::new(|this: &mut Self, idx| {
                            let c = CENTS_STEPS.get(idx).copied().unwrap_or(50.0);
                            this.cents_window = c;
                            let _ = this.tx.unbounded_send(Message::SetCents(c));
                        }),
                        options: CENTS_STEPS
                            .iter()
                            .map(|c| RadioItem {
                                label: format!("±{c:.0}¢"),
                                ..Default::default()
                            })
                            .collect(),
                    }
                    .into(),
                ],
                ..Default::default()
            }
            .into(),
            SubMenu {
                label: "Sustentação".into(),
                submenu: vec![
                    RadioGroup {
                        selected: sustain_idx,
                        select: Box::new(|this: &mut Self, idx| {
                            let s = SUSTAIN_STEPS.get(idx).copied().unwrap_or(500.0);
                            this.sustain_ms = s;
                            let _ = this.tx.unbounded_send(Message::SetSustain(s));
                        }),
                        options: SUSTAIN_STEPS
                            .iter()
                            .map(|s| RadioItem {
                                label: format!("{s:.0} ms"),
                                ..Default::default()
                            })
                            .collect(),
                    }
                    .into(),
                ],
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            CheckmarkItem {
                label: "Som".into(),
                checked: self.audible,
                activate: Box::new(|this: &mut Self| {
                    this.audible = !this.audible;
                    let _ = this.tx.unbounded_send(Message::SetAudible(this.audible));
                }),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Repetir tom".into(),
                activate: Box::new(|this: &mut Self| {
                    let _ = this.tx.unbounded_send(Message::Replay);
                }),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: "Limpar estatísticas".into(),
                activate: Box::new(|this: &mut Self| {
                    let _ = this.tx.unbounded_send(Message::ResetStats);
                }),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Restaurar padrões".into(),
                activate: Box::new(|this: &mut Self| {
                    let _ = this.tx.unbounded_send(Message::ResetDefaults);
                }),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Sair".into(),
                icon_name: "application-exit".into(),
                activate: Box::new(|_| std::process::exit(0)),
                ..Default::default()
            }
            .into(),
        ]
    }
}
