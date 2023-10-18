use crate::map::*;

pub struct Ghost {
    pub current_room: RoomLabel,
    pub ghost_room: RoomLabel,
    path_to_target: Option<Path>,
}

impl Ghost {
    pub fn new() -> Self {
        Ghost {
            current_room: 0,
            ghost_room: 7,
            path_to_target: None,
        }
    }

    pub fn move_room(&mut self, map: &Map) {
        println!("Moving ghost");
        let new_path = match self.path_to_target.clone() {
            None => {
                let target = self.next_target(map);
                Some(map.get_path(self.current_room, target))
            }
            Some(path) => {
                let mut path = path.clone();
                self.current_room = path.pop().unwrap();
                println!("Moved to room {}", self.current_room);
                if path.is_empty() {
                    None
                } else {
                    Some(path)
                }
            }
        };
        self.path_to_target = new_path;
    }

    fn target(&self) -> Option<RoomLabel> {
        self.path_to_target.clone().and_then(|p| p.first().copied())
    }

    pub fn has_evidence_type(&self, evidence: EvidenceType) -> bool {
        return true;
    }

    fn next_target(&self, map: &Map) -> RoomLabel {
        if self.current_room == self.ghost_room {
            let other_rooms: Vec<RoomLabel> = map
                .rooms
                .clone()
                .into_iter()
                .filter(|r| r.label != self.current_room)
                .map(|r| r.label)
                .collect();

            let mut rng = rand::thread_rng();
            let selected_ix = rand::Rng::gen_range(&mut rng, 0..other_rooms.len());
            other_rooms[selected_ix]
        } else {
            self.ghost_room
        }
    }
}

pub enum GhostType {
    Spirit,
    Poltergeist,
    Jinn,
    Mare,
    Revenant,
    Shade,
    Demon,
    Hantu,
    Myling,
    Onryo,
    Twins,
    Obake,
    Moroi,
    // Mimic
}

impl GhostType {
    pub fn has_evidence_type(&self, evidence: EvidenceType) -> bool {
        return true;
    }
}

pub enum EvidenceType {
    Emf,
    Ultraviolet,
    Freezing,
    GhostOrbs,
    Writing,
    SpiritBox,
}
