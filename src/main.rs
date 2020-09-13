use std::io::{Cursor, Write};
use std::sync::{Arc, Mutex};
use std::collections::HashMap;
use serialport::SerialPortType;
use hyper::{
    Request,
    Response,
    Body,
    body::HttpBody,
};

#[derive(Clone, Copy)]
struct Color(f32, f32, f32);

impl Color {
    pub fn from_hue(mut hue: f32) -> Color {
        hue = 6.0 * ((hue % 1. + 1.) % 1.);
        if hue < 1.0 {
            Color(1.0, hue, 0.0)
        } else if hue < 2.0 {
            hue -= 1.0;
            Color(1.0 - hue, 1.0, 0.0)
        } else if hue < 3.0 {
            hue -= 2.0;
            Color(0.0, 1.0, hue)
        } else if hue < 4.0 {
            hue -= 3.0;
            Color(0.0, 1.0 - hue, 1.0)
        } else if hue < 5.0 {
            hue -= 4.0;
            Color(hue, 0.0, 1.0)
        } else {
            hue -= 5.0;
            Color(1.0, 0.0, 1.0 - hue)
        }
    }

    pub fn as_byte_color(&self) -> (u8, u8, u8) {
        ((self.0 * 255.0) as u8, (self.1 * 255.0) as u8, (self.2 * 255.0) as u8)
    }
}

impl std::ops::Mul<Color> for f32 {
    type Output = Color;
    fn mul(self, value: Color) -> Color {
        Color(value.0 * self, value.1 * self, value.2 * self)
    }
}

impl std::ops::Add<Color> for Color {
    type Output = Color;
    fn add(self, value: Color) -> Color {
        Color(self.0 + value.0, self.1 + value.1, self.2 + value.2)
    }
}

enum ColorFormat {
    GRB,
}

impl ColorFormat {
    pub fn as_bytes(&self, colors: &[Color]) -> Box<[u8]> {
        match self {
            ColorFormat::GRB => {
                let mut bytes = Vec::with_capacity(colors.len() * 3);

                for c in colors {
                    let (r, g, b) = c.as_byte_color();
                    bytes.push(r);
                    bytes.push(g);
                    bytes.push(b);
                }

                return bytes.into_boxed_slice();
            },
        }
    }
}

enum Instruction<'a> {
    Show,
    Clear,
    SetPixelColor(u16, Color),
    SetPixelColorGamma(u16, Color),
    SetPixels(&'a [Color]),
}

impl Instruction<'_> {
    pub fn write(&self, w: &mut dyn Write) -> std::io::Result<()> {
        match self {
            Instruction::Show => w.write_all(&[0, 0]),
            Instruction::Clear => w.write_all(&[1, 0]),
            Instruction::SetPixelColor(i, col) => {
                let i_bytes = i.to_le_bytes();
                let (r, g, b) = col.as_byte_color();
                w.write_all(&[2, 0, i_bytes[0], i_bytes[1], r, g, b])
            },
            Instruction::SetPixelColorGamma(i, col) => {
                let i_bytes = i.to_le_bytes();
                let (r, g, b) = col.as_byte_color();
                w.write_all(&[3, 0, i_bytes[0], i_bytes[1], r, g, b])
            },
            Instruction::SetPixels(p) => {
                w.write_all(&[4, 0])?;
                w.write_all(&ColorFormat::GRB.as_bytes(p))?;
                Ok(())
            },
        }
    }
}

#[derive(serde::Deserialize)]
struct AuthState {
    pub token: String,
}

#[derive(serde::Deserialize)]
struct TeamInfo {
    pub consecutive_round_losses: i32,
    pub matches_won_this_series: i32,
    pub name: Option<String>,
    pub score: i32,
    pub timeouts_remaining: i32,
}

#[derive(serde::Deserialize)]
struct MapState {
    pub current_spectators: i32,
    pub mode: String,
    pub name: String,
    pub num_matches_to_win_series: i32,
    pub phase: String,
    pub round: i32,
    pub round_wins: Option<HashMap<String, String>>,
    pub souvenirs_total: i32,
    pub team_ct: TeamInfo,
    pub team_t: TeamInfo,
}

#[derive(serde::Deserialize)]
struct MatchStats {
    pub assists: i32,
    pub deaths: i32,
    pub kills: i32,
    pub mvps: i32,
    pub score: i32,
}

#[derive(serde::Deserialize)]
struct PlayerState {
    pub armor: f32,
    pub burning: f32,
    pub equip_value: i32,
    pub flashed: f32,
    pub health: f32,
    pub helmet: bool,
    pub money: i32,
    pub round_killhs: i32,
    pub round_kills: i32,
    pub smoked: f32,
}

#[derive(Clone, serde::Deserialize)]
struct Weapon {
    pub ammo_clip: Option<i32>,
    pub ammo_clip_max: Option<i32>,
    pub ammo_reserve: Option<i32>,
    pub name: String,
    pub paintkit: String,
    pub state: String,
    pub r#type: String,
}

#[derive(serde::Deserialize)]
struct Player {
    pub activity: String,
    pub clan: Option<String>,
    pub match_stats: Option<MatchStats>,
    pub name: String,
    pub observer_slot: Option<i32>,
    pub state: PlayerState,
    pub steamid: String,
    pub team: String,
    pub weapons: HashMap<String, Weapon>,
}

#[derive(serde::Deserialize)]
struct ProviderState {
    pub appid: i32,
    pub name: String,
    pub steamid: String,
    pub timestamp: u64,
    pub version: i32,
}

#[derive(serde::Deserialize)]
struct RoundState {
    pub bomb: Option<String>,
    pub phase: String,
    pub win_team: Option<String>,
}

#[derive(serde::Deserialize)]
#[serde(default)]
struct GameState {
    pub auth: Option<AuthState>,
    pub map: Option<MapState>,
    pub player: Option<Player>,
    pub provider: Option<ProviderState>,
    pub round: Option<RoundState>,
    pub previously: Option<HashMap<String, serde_json::Value>>,
}

impl GameState {
    pub fn active_weapon(&self) -> Option<(&str, &Weapon)> {
        if let Some(player) = &self.player {
            for (k, w) in &player.weapons {
                let w: &Weapon = w;
                if w.state == "active" || w.state == "reloading" {
                    return Some((k, w));
                }
            }
        }
        None
    }
}

impl Default for GameState {
    fn default() -> GameState {
        GameState {
            auth: None,
            map: None,
            player: None,
            provider: None,
            round: None,
            previously: None,
        }
    }
}

fn clear(cols: &mut [Color]) {
    for i in 0..cols.len() {
        cols[i] = Color(0., 0., 0.);
    }
}

fn fill(cols: &mut [Color], col: Color, alpha: f32) {
    for i in 0..cols.len() {
        cols[i] = (1. - alpha) * cols[i] + alpha * col;
    }
}

fn draw_line(cols: &mut [Color], from: f32, to: f32, col: Color) {
    for i in usize::max(f32::floor(from) as usize, 0) .. usize::min(f32::ceil(to) as usize, cols.len()) {
        let amt = f32::min(f32::max(i as f32 + 1.0 - from, 0.0), 1.0)
            + f32::min(f32::max(to - i as f32, 0.0), 1.0)
            - 1.0;

        cols[i] = (1. - amt) * cols[i] + amt * col;
    }
}

fn merge(a: &mut serde_json::Value, b: serde_json::Value) {
    match (a, b) {
        (a @ &mut serde_json::Value::Object(_), serde_json::Value::Object(b)) => {
            let a = a.as_object_mut().unwrap();
            for (k,v) in b {
                merge(a.entry(k).or_insert(serde_json::Value::Null), v.clone());
            }
        },
        (a, b) => *a = b,
    }
}

async fn handle_http(mut req: Request<Body>, state: Arc<Mutex<GameState>>, next_event: Arc<Mutex<Option<EventType>>>) -> Result<Response<Body>, std::convert::Infallible> {
    let mut bytes = Vec::with_capacity(req.body().size_hint().lower() as usize);
    loop {
        if let Some(Ok(data)) = req.body_mut().data().await {
            bytes.extend_from_slice(&*data);
        } else {
            break;
        }
    }

    //let value: serde_json::Value = serde_json::from_reader(std::io::BufReader::new(Cursor::new(bytes))).unwrap();
    {
        let mut guard = state.lock().unwrap();
        *guard = serde_json::from_reader(std::io::BufReader::new(Cursor::new(bytes))).unwrap();

        if let Some(map) = &(*guard).previously {
            if let Some((k, w)) = (*guard).active_weapon() {
                if let Some(ammo_clip) = w.ammo_clip {
                    if let Some(prev_player) = map.get("player") {
                        if let Some(prev_weapons) = prev_player.get("weapons") {
                            if let Some(prev_weapon) = prev_weapons.get(k) {
                                if prev_weapon.get("state").is_none() {
                                    if let Some(prev_ammo) = prev_weapon.get("ammo_clip") {
                                        if ammo_clip < prev_ammo.as_i64().unwrap() as i32 {
                                            *next_event.lock().unwrap() = Some(EventType::Shoot);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

    }

    let response = Response::new(Body::empty());
    Ok(response)
}

fn do_rainbow(cols: &mut [Color], time: f64, cycle_time: f64, alpha: f32) {
    let cycle = (((time / cycle_time) % 1. + 1.) % 1.) as f32;
    let len = cols.len();
    for i in 0..len {
        cols[i] = (1. - alpha) * cols[i] + alpha * Color::from_hue(cycle - (i as f32 / len as f32));
    }
}

type Event = (EventType, f64);

#[derive(Clone, Copy)]
enum EventType {
    Shoot,
    Kill,
    KnifeKill,
}

fn do_lights(serial: &str, state: Arc<Mutex<GameState>>, next_event: Arc<Mutex<Option<EventType>>>) {
    let start = std::time::Instant::now();

    let mut last_event: Option<Event> = None;

    let mut serial = serialport::open_with_settings(serial, &serialport::SerialPortSettings {
        baud_rate: 250000,
        data_bits: serialport::DataBits::Eight,
        flow_control: serialport::FlowControl::None,
        parity: serialport::Parity::None,
        stop_bits: serialport::StopBits::One,
        timeout: std::time::Duration::from_millis(100),
    }).expect("Failed to open serial port");

    let mut cols = [Color(0.0, 0.0, 1.0); 60];
    let s = &mut serial;
    loop {
        let time_now = (std::time::Instant::now() - start).as_secs_f64();
        {
            let mut guard = next_event.lock().unwrap();
            if let Some(e) = &*guard {
                last_event = Some((*e, time_now));
                *guard = None;
            }
        }

        {
            let guard = state.lock().unwrap();
            let state: &GameState = &*guard;
            if let Some((_k, w)) = state.active_weapon() {
                if w.r#type == "Knife" {
                    let cycle = (time_now % 1. + 1.) % 1.;
                    let amt = if cycle < 0.25 {
                        0.5 - cycle * 2.
                    } else if cycle < 0.5 {
                        0.5 - cycle
                    } else {
                        0.
                    };

                    do_rainbow(&mut cols, time_now, 1., amt as f32);
                } else if let Some(ammo_clip) = w.ammo_clip {
                    let ammo = (ammo_clip as f64 / w.ammo_clip_max.unwrap() as f64) as f32;

                    clear(&mut cols);
                    do_rainbow(&mut cols, time_now, 1., 0.5);
                    let len = cols.len();
                    draw_line(&mut cols, len as f32 * ammo, len as f32, Color(0., 0., 0.))
                }
            }

            if let Some((event, time)) = &last_event {
                let since = time_now - time;
                match event {
                    EventType::Shoot => {
                        fill(&mut cols, Color(1., 1., 0.25), (1.0 - since * 8.).max(0.) as f32);
                    },
                    _ => (),
                }
            }
        }
        Instruction::SetPixels(&cols).write(s).unwrap();
        Instruction::Show.write(s).unwrap();
    }
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let ps = serialport::available_ports().expect("Failed to get serial ports");

    for i in 0..ps.len() {
        let port = &ps[i];
        println!("{}: {}", i, match &port.port_type {
            SerialPortType::UsbPort(info) => match &info.product {
                Some(p) => format!("{}, ({})", p, port.port_name),
                _ => port.port_name.clone(),
            },
            _ => port.port_name.clone(),
        });
    }

    let port_name = loop {
        let mut s = String::new();
        std::io::stdin().read_line(&mut s).expect("Failed to read input");
        if let Ok(i) = s.trim().parse::<usize>() {
            if let Some(p) = ps.get(i) {
                break p.port_name.clone();
            }
            println!("No index");
        } 
        println!("Enter a valid index");
    };
    println!("Beginning to send data on {}", port_name);

    let state = Arc::new(Mutex::new(GameState::default()));
    let next_event = Arc::new(Mutex::new(None));

    let s1 = Arc::clone(&state);
    let e1 = Arc::clone(&next_event);
    let s2 = Arc::clone(&state);
    let e2 = Arc::clone(&next_event);

    std::thread::spawn(move || {
        do_lights(&port_name, s2, e2);
    });

    if let Err(e) = hyper::Server::bind(&std::net::SocketAddr::from(([127, 0, 0, 1], 3000))).serve(hyper::service::make_service_fn(|_conn| {
        let s1 = Arc::clone(&s1);
        let e1 = Arc::clone(&e1);
        async {
            Ok::<_, std::convert::Infallible>(hyper::service::service_fn(move |req| {
                let s1 = Arc::clone(&s1);
                let e1 = Arc::clone(&e1);
                handle_http(req, s1, e1)
            }))
        }
    })).await {
        eprintln!("Server error: {}", e);
    }
    Ok(())
}
