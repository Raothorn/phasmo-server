use crate::{ghost::*, map::*, server::Handle};
use log::info;
use rand::Rng;
use serde::Serialize;
use std::{net::SocketAddr, sync::Arc};
use tokio::{sync::mpsc::Sender, time::Duration};

#[derive(Serialize, Clone)]
pub struct Player {
    pub name: String,
    pub addr: SocketAddr,
    pub last_loc: Option<RoomLabel>,
    sanity: u32,
}

#[derive(Serialize)]
pub enum GameUpdate {
    Lobby {
        players: Vec<String>,
    },
    Sim {
        players: Vec<Player>,
        ghost_location: RoomLabel,
        favorite_room: RoomLabel,
        ghost_orbs_visible: bool,
        ambient_temp: i32,
        ghost_room_temp: i32,
        notifications: Vec<String>
    },
}

#[derive(Clone)]
pub enum EventTrigger {
    RemoveGhostOrbs,
    UpdateThermometer,
    EndEMF,
}

pub struct Simulation {
    pub players: Vec<Player>,
    pub started: bool,
    event_triggers: Vec<(Duration, EventTrigger)>,
    ghost: Ghost,
    map: Map,
    cur_time: Duration,
    flags: SimFlags,
    options: SimOptions,
    notify_queue: Vec<String>
}


impl Simulation {
    pub fn new() -> Self {
        let mut event_triggers = Vec::new();
        event_triggers.push((Duration::from_secs(0), EventTrigger::UpdateThermometer));
        Simulation {
            players: Vec::new(),
            started: false,
            event_triggers,
            ghost: Ghost::new(),
            map: Map::new(),
            cur_time: Duration::from_secs(0),
            flags: SimFlags::new(),
            options: SimOptions::new(),
            notify_queue: Vec::new()
        }
    }

    pub fn add_player(&mut self, addr: SocketAddr, name: &str) -> Result<(), String> {
        let players = &mut self.players;
        if players.iter().any(|p| p.addr == addr) {
            Err("Already connected".to_owned())
        } else if players.iter().any(|p| p.name == name) {
            Err("Name taken".to_owned())
        } else {
            info!("Adding player {} to lobby", name);
            let player = Player {
                name: name.to_owned(),
                addr,
                last_loc: None,
                sanity: 100,
            };
            players.push(player);
            Ok(())
        }
    }

    pub fn update_player_loc(&mut self, name: &str, location: RoomLabel) {
        let mut player = self.players.iter_mut().find(|p| p.name == name);
        if let Some(player) = player.as_mut() {
            player.last_loc = Some(location);
        }
    }

    pub fn start(&mut self) {
        self.started = true;
    }

    // REAL TIME UPDATES
    pub fn update(&mut self, dt: Duration) -> bool {
        self.cur_time += dt;

        let mut changed = false;
        let move_elapse = self.cur_time - self.flags.last_ghost_move;
        if move_elapse > self.options.ghost_move_interval {
            self.ghost.move_room(&self.map);
            self.flags.last_ghost_move = self.cur_time;
            changed = true;
        }

        let pulse_elapse = self.cur_time - self.flags.last_event_pulse;
        if pulse_elapse > self.options.event_pulse_interval {
            self.event_pulse(self.cur_time);
            self.flags.last_event_pulse = self.cur_time;
            changed = true;
        }

        let (triggered, remaining): (Vec<_>, Vec<_>) = self
            .event_triggers
            .clone()
            .into_iter()
            .partition(|(time, _)| *time < self.cur_time);

        self.event_triggers = remaining;

        for (_, trigger) in triggered {
            changed = true;
            match trigger {
                EventTrigger::RemoveGhostOrbs => {
                    println!("Orbs no longer visible");
                    self.flags.orbs_visible = false;
                }
                EventTrigger::UpdateThermometer => {
                    // only exists to force a periodic update
                    let event_time = self.cur_time + self.options.thermometer_update_interval;
                    self.event_triggers
                        .push((event_time, EventTrigger::UpdateThermometer));
                }
                EventTrigger::EndEMF => self.flags.emf_level = 0,
            }
        }
        return changed;
    }

    fn event_pulse(&mut self, cur_time: Duration) {
        println!("Event pulse");
        let mut rng = rand::thread_rng();
        if !self.flags.orbs_visible {
            let x: f64 = Rng::gen_range(&mut rng, 0.0..1.0);
            if x < self.options.ghost_orbs_frequency {
                println!("Orbs now visible");
                self.flags.orbs_visible = true;

                let trigger_time = cur_time + self.options.ghost_orbs_duration;
                self.event_triggers
                    .push((trigger_time, EventTrigger::RemoveGhostOrbs));
            }
        }

        // Ghost interaction
        if Rng::gen_range(&mut rng, 0.0..1.0) < self.options.ghost_interaction_frequency {
            println!("Interaction");
            let interaction =
                GhostInteractionType::generate(self.ghost.has_evidence_type(EvidenceType::Writing));

            let min_emf = 2; 
            let max_emf = if self.ghost.has_evidence_type(EvidenceType::Emf) {
                5
            } else {
                3
            };
            self.blast_emf(min_emf, max_emf);

            let room = self.ghost.current_room;
            let msg = match interaction {
                GhostInteractionType::Sound => {
                    format!("Play generic sound in room {}", room)
                },
                GhostInteractionType::LightsFlicker => {
                    format!("Flicker lights in room {}", room)
                },
                GhostInteractionType::GhostWriting => {
                    format!("Play ghost writing sound in room {}", room)
                },
            };
            self.notify(&msg);
        }
    }

    pub fn get_gameupdate(&self) -> GameUpdate {
        let player_names = self.players.iter().map(|p| p.name.clone()).collect();

        if !self.started {
            GameUpdate::Lobby {
                players: player_names,
            }
        } else {
            let mut rng = rand::thread_rng();

            let v = self.options.temperature_variability;
            let amb_noise = Rng::gen_range(&mut rng, -v..v);
            let gr_noise = Rng::gen_range(&mut rng, -v..v);

            // TODO magic number
            let ambient_temp = std::cmp::max(self.flags.ambient_temp + amb_noise, 40);

            let ghost_room_temp = self.ghost_room_temp() + gr_noise;
            let ghost_room_temp = if self.ghost.has_evidence_type(EvidenceType::Freezing) {
                ghost_room_temp
            } else {
                std::cmp::max(ghost_room_temp, self.flags.ghost_room_min_temp)
            };

            GameUpdate::Sim {
                players: self.players.clone(),
                ghost_location: self.ghost.current_room,
                favorite_room: self.ghost.ghost_room,
                ghost_orbs_visible: self.flags.orbs_visible,

                ghost_room_temp,
                ambient_temp,
                notifications: self.notify_queue.clone(),
            }
        }
    }

    fn ghost_room_temp(&self) -> i32 {
        let mins = self.cur_time.as_secs() / 5;
        let gr_temp =
            self.flags.ambient_temp + (self.flags.delta_temp * mins.try_into().unwrap_or(0));
        std::cmp::max(gr_temp, self.flags.ghost_room_min_temp)
    }

    fn blast_emf(&mut self, min_amount: u32, max_amount: u32) {
        if self.flags.emf_level == 0 {
            return;
        }

        let mut rng = rand::thread_rng();
        self.flags.emf_level = Rng::gen_range(&mut rng, min_amount..=max_amount);

        let event_time = self.cur_time + self.options.emf_blast_duration;
        self.event_triggers.push((event_time, EventTrigger::EndEMF));
    }

    fn notify(&mut self, msg: &str) {
        self.notify_queue.push(msg.to_owned());
    }

    pub fn clear_notify_queue(&mut self) {
        self.notify_queue.clear();
    }
}

pub struct SimFlags {
    last_event_pulse: Duration,
    last_ghost_move: Duration,

    emf_level: u32,
    ghost_type: GhostType,

    // Temp
    ghost_room_min_temp: i32,
    delta_temp: i32,
    ambient_temp: i32,

    // Ghost orbs
    orbs_visible: bool,
}

impl SimFlags {
    fn new() -> Self {
        let mut rng = rand::thread_rng();

        let ghost_type = GhostType::Spirit;

        let ambient_temp = 50;
        let ghost_room_min_temp = if ghost_type.has_evidence_type(EvidenceType::Freezing) {
            28
        } else {
            35
        };

        let mins_to_min_temp = rand::Rng::gen_range(&mut rng, 4..10);
        // change in temperature per minute
        let delta_temp = (ghost_room_min_temp - ambient_temp) / mins_to_min_temp;

        SimFlags {
            last_ghost_move: Duration::from_secs(0),
            last_event_pulse: Duration::from_secs(0),
            emf_level: 0,
            ghost_type,

            ambient_temp,
            ghost_room_min_temp,
            delta_temp,

            orbs_visible: false,
        }
    }
}

pub struct SimOptions {
    ghost_move_interval: Duration,
    event_pulse_interval: Duration,
    ghost_orbs_duration: Duration,
    ghost_orbs_frequency: f64,
    temperature_variability: i32,
    thermometer_update_interval: Duration,

    ghost_interaction_frequency: f64,
    ghost_event_frequency: f64,

    emf_blast_duration: Duration
}

impl SimOptions {
    fn new() -> Self {
        SimOptions {
            ghost_move_interval: Duration::from_secs(30),
            event_pulse_interval: Duration::from_secs(60),

            ghost_orbs_duration: Duration::from_secs(20),
            ghost_orbs_frequency: 1.0,

            temperature_variability: 5,
            thermometer_update_interval: Duration::from_secs(2),

            ghost_interaction_frequency: 0.75,
            ghost_event_frequency: 0.25,
            emf_blast_duration: Duration::from_secs(2)
        }
    }

    // Load from admin options
    fn load() -> Self {
        todo!()
    }
}

enum GhostInteractionType {
    Sound,
    LightsFlicker,
    GhostWriting,
}

impl GhostInteractionType {
    fn generate(writing: bool) -> Self {
        let mut rng = rand::thread_rng();
        let x = Rng::gen_range(&mut rng, 0.0..1.0);

        if writing {
            if x < 0.33 {
                GhostInteractionType::Sound
            } else if x < 0.66 {
                GhostInteractionType::LightsFlicker
            } else {
                GhostInteractionType::GhostWriting
            }
        } else {
            if x < 0.5 {
                GhostInteractionType::Sound
            } else {
                GhostInteractionType::LightsFlicker
            }
        }
    }
}
