use std::collections::HashSet;

pub type RoomLabel = usize;
pub type Path = Vec<RoomLabel>;

#[derive(Clone)]
pub struct Room {
    pub label: RoomLabel,
    connected_rooms: Vec<RoomLabel>,
}

pub struct Map {
    pub rooms: Vec<Room>,
}

impl Map {
    pub fn new() -> Self {
        Map {
            rooms: vec! [
                Room {
                    label: 0,
                    connected_rooms: vec![1, 2, 13],
                },
                Room {
                    label: 1,
                    connected_rooms: vec![0],
                },
                Room {
                    label: 2,
                    connected_rooms: vec![0, 3, 4, 6],
                },
                Room {
                    label: 3,
                    connected_rooms: vec![2, 4],
                },
                Room {
                    label: 4,
                    connected_rooms: vec![2, 3, 5],
                },
                Room {
                    label: 5,
                    connected_rooms: vec![4],
                },
                Room {
                    label: 6,
                    connected_rooms: vec![2,7],
                },
                Room {
                    label: 7,
                    connected_rooms: vec![6],
                },
                Room {
                    label: 8,
                    connected_rooms: vec![9],
                },
                Room {
                    label: 9,
                    connected_rooms: vec![8, 13],
                },
                Room {
                    label: 10,
                    connected_rooms: vec![11,13,9],
                },
                Room {
                    label: 11,
                    connected_rooms: vec![10],
                },
                Room {
                    label: 12,
                    connected_rooms: vec![13],
                },
                Room {
                    label: 13,
                    connected_rooms: vec![0, 9, 10, 12],
                },
            ]
        }
    }

    pub fn get_path(&self, from: RoomLabel, to: RoomLabel) -> Path {
        let mut path = self._get_path(from, to, Vec::new(), HashSet::new()).unwrap();
        path.reverse();
        path
    }

    // basic DFS to get ghost path
    fn _get_path(
        &self,
        room: RoomLabel,
        target: RoomLabel,
        path: Vec<RoomLabel>,
        visited: HashSet<RoomLabel>,
    ) -> Option<Path> {
        if room == target {
            Some(path.clone())
        } else {
            let room = &self.rooms[room];
            let mut paths = Vec::new();
            for adj in &room.connected_rooms {
                if !visited.contains(&adj) {
                    let mut new_path = path.clone();
                    let mut new_visited = visited.clone();
                    new_path.push(*adj);
                    new_visited.insert(*adj);
                    let shortest_path = self._get_path(*adj, target, new_path, new_visited);
                    paths.push(shortest_path);
                }
            }

            if paths.is_empty() {
                None
            } else {
                let mut shortest_path: Vec<usize> = (0..self.rooms.len() + 1).into_iter().collect();
                for path in paths {
                    match path {
                        None => (),
                        Some(path) => {
                            if path.len() < shortest_path.len() {
                                shortest_path = path;
                            }
                        }
                    }
                }
                Some(shortest_path)
            }
        }
    }
}
