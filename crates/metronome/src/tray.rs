//! System-tray menu: toggle overlay/run, sound/flash, tempo source, BPM, bar.

use futures::channel::mpsc::UnboundedSender;

use crate::Message;
use crate::config::Source;

/// Beats-per-bar choices offered in the "Compasso" submenu.
pub const BAR_OPTIONS: [u32; 5] = [1, 2, 3, 4, 6];

pub struct MetronomeTray {
    pub tx: UnboundedSender<Message>,
    pub enabled: bool,
    pub running: bool,
    pub audible: bool,
    pub flash: bool,
    pub source: Source,
    pub bpm: f64,
    pub beats_per_bar: u32,
}

impl ksni::Tray for MetronomeTray {
    fn icon_name(&self) -> String {
        "media-playback-start".into()
    }
    fn title(&self) -> String {
        "sceno · metronome".into()
    }
    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        use ksni::menu::*;
        let source = self.source;
        let bar_selected = BAR_OPTIONS
            .iter()
            .position(|&n| n == self.beats_per_bar)
            .unwrap_or(3);

        let mut items: Vec<ksni::MenuItem<Self>> = vec![
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
            CheckmarkItem {
                label: "Tocar".into(),
                checked: self.running,
                activate: Box::new(|this: &mut Self| {
                    this.running = !this.running;
                    let _ = this.tx.unbounded_send(Message::SetRunning(this.running));
                }),
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
            CheckmarkItem {
                label: "Flash".into(),
                checked: self.flash,
                activate: Box::new(|this: &mut Self| {
                    this.flash = !this.flash;
                    let _ = this.tx.unbounded_send(Message::SetFlash(this.flash));
                }),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            SubMenu {
                label: "Fonte".into(),
                submenu: vec![
                    RadioGroup {
                        selected: source.index(),
                        select: Box::new(|this: &mut Self, idx| {
                            this.source = Source::from_idx(idx);
                            let _ = this.tx.unbounded_send(Message::SetSource(this.source));
                        }),
                        options: vec![
                            RadioItem {
                                label: Source::Manual.label().into(),
                                ..Default::default()
                            },
                            RadioItem {
                                label: Source::Song.label().into(),
                                ..Default::default()
                            },
                            RadioItem {
                                label: Source::Detect.label().into(),
                                ..Default::default()
                            },
                        ],
                    }
                    .into(),
                ],
                ..Default::default()
            }
            .into(),
            SubMenu {
                label: "Andamento".into(),
                submenu: vec![
                    StandardItem {
                        label: format!("Atual: {:.0} BPM", self.bpm),
                        enabled: false,
                        ..Default::default()
                    }
                    .into(),
                    StandardItem {
                        label: "− 5 BPM".into(),
                        activate: Box::new(|this: &mut Self| {
                            let _ = this.tx.unbounded_send(Message::AdjustBpm(-5.0));
                        }),
                        ..Default::default()
                    }
                    .into(),
                    StandardItem {
                        label: "+ 5 BPM".into(),
                        activate: Box::new(|this: &mut Self| {
                            let _ = this.tx.unbounded_send(Message::AdjustBpm(5.0));
                        }),
                        ..Default::default()
                    }
                    .into(),
                    StandardItem {
                        label: "Tap tempo".into(),
                        activate: Box::new(|this: &mut Self| {
                            let _ = this.tx.unbounded_send(Message::Tap);
                        }),
                        ..Default::default()
                    }
                    .into(),
                ],
                ..Default::default()
            }
            .into(),
            SubMenu {
                label: "Compasso".into(),
                submenu: vec![
                    RadioGroup {
                        selected: bar_selected,
                        select: Box::new(|this: &mut Self, idx| {
                            let n = BAR_OPTIONS.get(idx).copied().unwrap_or(4);
                            this.beats_per_bar = n;
                            let _ = this.tx.unbounded_send(Message::SetBeatsPerBar(n));
                        }),
                        options: BAR_OPTIONS
                            .iter()
                            .map(|n| RadioItem {
                                label: format!("{n}/4"),
                                ..Default::default()
                            })
                            .collect(),
                    }
                    .into(),
                ],
                ..Default::default()
            }
            .into(),
        ];

        // A per-song phase nudge only makes sense when following the song grid.
        if source == Source::Song {
            items.push(
                SubMenu {
                    label: "Sincronia".into(),
                    submenu: vec![
                        StandardItem {
                            label: "− 100 ms".into(),
                            activate: Box::new(|this: &mut Self| {
                                let _ = this.tx.unbounded_send(Message::NudgeOffset(-100));
                            }),
                            ..Default::default()
                        }
                        .into(),
                        StandardItem {
                            label: "+ 100 ms".into(),
                            activate: Box::new(|this: &mut Self| {
                                let _ = this.tx.unbounded_send(Message::NudgeOffset(100));
                            }),
                            ..Default::default()
                        }
                        .into(),
                        StandardItem {
                            label: "Limpar".into(),
                            activate: Box::new(|this: &mut Self| {
                                let _ = this.tx.unbounded_send(Message::ClearOffset);
                            }),
                            ..Default::default()
                        }
                        .into(),
                    ],
                    ..Default::default()
                }
                .into(),
            );
        }

        items.push(MenuItem::Separator);
        items.push(
            StandardItem {
                label: "Sair".into(),
                icon_name: "application-exit".into(),
                activate: Box::new(|_| std::process::exit(0)),
                ..Default::default()
            }
            .into(),
        );
        items
    }
}
