use crate::utils;
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
    sanity: f64,
}

impl Player {
    fn drain_sanity(&mut self, amt: f64) {
        let new_amt = self.sanity - amt;
        self.sanity = if new_amt < 0.0 { 0.0 } else { new_amt };
    }
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
        emf_level: u32,
        notifications: Vec<String>,
        ghost_writing_visible: bool,
    },
}

#[derive(Clone)]
pub enum EventTrigger {
    RemoveGhostOrbs,
    UpdateThermometer,
    EndEMF,
    EndHunt,
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
    notify_queue: Vec<String>,
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
            notify_queue: Vec::new(),
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
                sanity: 100.0,
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
        // if self.flags.is_hunting {
        //     self.check_triggers();
        // }

        // Drain everyone's sanity
        let millis: u32 = dt.as_millis().try_into().unwrap();
        let millis_f: f64 = millis.try_into().unwrap();
        let seconds = millis_f / 1000.0;
        let sanity_drain = self.options.sanity_drain_rate * seconds;

        for player in self.players.iter_mut() {
            player.drain_sanity(sanity_drain);
        }

        let mut changed = false;
        let move_elapse = self.cur_time - self.flags.last_ghost_move;
        if move_elapse > self.options.ghost_move_interval {
            self.flags.last_ghost_move = self.cur_time;
            self.move_ghost();
            changed = true;
        }

        let pulse_elapse = self.cur_time - self.flags.last_event_pulse;
        if pulse_elapse > self.options.event_pulse_interval {
            self.event_pulse(self.cur_time);
            self.flags.last_event_pulse = self.cur_time;
            changed = true;
        }

        let changed = self.check_triggers() || changed;
        return changed;
    }
    
    fn check_triggers(&mut self) -> bool {
        let mut changed = false;

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
                    println!("Current time: {}", self.cur_time.as_secs());

                    // only exists to force a periodic update
                    let event_time = self.cur_time + self.options.thermometer_update_interval;
                    self.event_triggers
                        .push((event_time, EventTrigger::UpdateThermometer));
                }
                EventTrigger::EndEMF => self.flags.emf_level = 0,
                EventTrigger::EndHunt => {
                    self.flags.is_hunting = false;
                },
            }
        }
        return changed;

    }

    fn move_ghost(&mut self) {
        // chance to just stay in ghost room
        // TODO parameterize tendency to stay in ghost room
        let stay = self.ghost.current_room == self.ghost.ghost_room && utils::roll(0.5);
        if !stay {
            self.ghost.move_room(&self.map)
        }

        if let Some(book_room) = self.flags.book_location {
            if !self.flags.ghost_writing_visible
                && self.ghost.current_room == book_room
                && utils::roll(self.options.ghost_interaction_frequency)
                {
                    self.flags.ghost_writing_visible = true
                }
        }
    }

    fn event_pulse(&mut self, cur_time: Duration) {
        println!("Event pulse");

        // Chance for hunt
        // let hunt_chance = self.options.ghost_hunt_frequency + self.average_sanity_drain();
        // if utils::roll(hunt_chance) {
        //     self.flags.is_hunting = true;
        // 
        //     let time = self.cur_time + self.options.ghost_hunt_duration;
        //     self.event_triggers.push((time, EventTrigger::EndHunt));
        //     // if hunt occurs, no other events need to occur
        //     return;
        // }
        

        // Chance for orbs
        if !self.flags.orbs_visible {
            if utils::roll(self.options.ghost_orbs_frequency) {
                println!("Orbs now visible");
                self.flags.orbs_visible = true;

                let trigger_time = cur_time + self.options.ghost_orbs_duration;
                self.event_triggers
                    .push((trigger_time, EventTrigger::RemoveGhostOrbs));
            }
        }

        // Chance for ghost interaction
        // Todo: multiplier
        let interaction_chance =
            self.options.ghost_interaction_frequency + (self.average_sanity_drain());
        let interaction_chance = 1.0;
        if true {
            println!("Interaction");
            // && book is in ghost current room
            let interaction = InteractionType::generate_interaction();

            // drain player's sanity
            for player in self.players.iter_mut() {
                // TODO parameterize
                let sanity_loss = if player.last_loc == Some(self.ghost.current_room) {
                    15.0
                } else {
                    5.0
                };
                player.drain_sanity(sanity_loss);
            }

            let min_emf = 2;
            let max_emf = if self.ghost.has_evidence_type(EvidenceType::Emf) {
                5
            } else {
                3
            };
            self.blast_emf(min_emf, max_emf);

            let room = self.ghost.current_room;
            let msg = interaction.interaction_msg();
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


                emf_level: self.flags.emf_level,
                ghost_room_temp,
                ambient_temp,
                notifications: self.notify_queue.clone(),
                ghost_writing_visible: self.flags.ghost_writing_visible
            }
        }
    }

    fn average_sanity_drain(&self) -> f64 {
        if self.players.is_empty() {
            return 0.0;
        }

        let players: u32 = self.players.len().try_into().unwrap();
        let players: f64 = players.into();

        let total: f64 = self.players.clone().into_iter().map(|p| p.sanity).sum();

        return total / players;
    }

    fn ghost_room_temp(&self) -> i32 {
        let mins = self.cur_time.as_secs() / 5;
        let gr_temp =
            self.flags.ambient_temp + (self.flags.delta_temp * mins.try_into().unwrap_or(0));
        std::cmp::max(gr_temp, self.flags.ghost_room_min_temp)
    }

    fn blast_emf(&mut self, min_amount: u32, max_amount: u32) {
        if self.flags.emf_level != 0 {
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
    book_location: Option<RoomLabel>,
    ghost_writing_visible: bool,

    is_hunting: bool,
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
            book_location: None,
            ghost_writing_visible: false,
            is_hunting: false
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

    emf_blast_duration: Duration,

    ghost_hunt_frequency: f64,
    ghost_hunt_duration: Duration,

    sanity_drain_rate: f64,
}

impl SimOptions {
    fn new() -> Self {
        SimOptions {
            ghost_move_interval: Duration::from_secs(10),
            event_pulse_interval: Duration::from_secs(10),

            ghost_orbs_duration: Duration::from_secs(20),
            ghost_orbs_frequency: 1.0,

            temperature_variability: 5,
            thermometer_update_interval: Duration::from_secs(2),

            ghost_interaction_frequency: 1.25,
            ghost_event_frequency: 1.25,
            ghost_hunt_frequency: 0.0,
            ghost_hunt_duration: Duration::from_secs(30),
            emf_blast_duration: Duration::from_secs(3),


            sanity_drain_rate: 0.05, // %/s
        }
    }

    // Load from admin options
    fn load() -> Self {
        todo!()
    }
}

#[derive(Clone)]
enum InteractionType {
    Sound,
    LightsFlicker,
    // GhostWriting,
}

impl InteractionType {
    fn generate_interaction() -> Self {
        let list = vec![InteractionType::Sound, InteractionType::LightsFlicker];
        utils::rng_select(&list)
    }

    fn interaction_msg(&self) -> String {
        let str = match self {
            InteractionType::Sound => "Sound",
            InteractionType::LightsFlicker => "Lights",
        };
        str.to_owned()
    }
}
