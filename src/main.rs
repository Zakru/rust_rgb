use std::io::{Cursor, Write};
use std::sync::{Arc, Mutex};
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
        hue = 6.0 * (hue % 1.0);
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

async fn handle_http(mut req: Request<Body>, cols: Arc<Mutex<[Color]>>, serial: Arc<Mutex<Box<dyn serialport::SerialPort>>>) -> Result<Response<Body>, std::convert::Infallible> {
    let mut bytes = Vec::with_capacity(req.body().size_hint().lower() as usize);
    loop {
        if let Some(Ok(data)) = req.body_mut().data().await {
            bytes.extend_from_slice(&*data);
        } else {
            break;
        }
    }
    let value: serde_json::Value = serde_json::from_reader(std::io::BufReader::new(Cursor::new(bytes))).unwrap();

    println!("{}", value);

    let cols = &mut *cols.lock().unwrap();
    let serial = &mut *serial.lock().unwrap();

    let response = Response::new(Body::empty());
    Ok(response)
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let ps = serialport::available_ports()?;

    for port in &ps {
        println!("{}", match &port.port_type {
            SerialPortType::UsbPort(info) => match &info.product {
                Some(p) => format!("{}, ({})", p, port.port_name),
                _ => port.port_name.clone(),
            },
            _ => port.port_name.clone(),
        });
    }

    let serial = serialport::open_with_settings(&ps[0].port_name, &serialport::SerialPortSettings {
        baud_rate: 250000,
        data_bits: serialport::DataBits::Eight,
        flow_control: serialport::FlowControl::None,
        parity: serialport::Parity::None,
        stop_bits: serialport::StopBits::One,
        timeout: std::time::Duration::from_millis(100),
    })?;

    let serial_arc = Arc::new(Mutex::new(serial));
    let cols = Arc::new(Mutex::new([Color(0.0, 0.0, 1.0); 60]));

    if let Err(e) = hyper::Server::bind(&std::net::SocketAddr::from(([127, 0, 0, 1], 3000))).serve(hyper::service::make_service_fn(|_conn| {
        let c = Arc::clone(&cols);
        let s = Arc::clone(&serial_arc);
        async {
            Ok::<_, std::convert::Infallible>(hyper::service::service_fn(move |req| {
                let c1 = Arc::clone(&c);
                let s1 = Arc::clone(&s);
                handle_http(req, c1, s1)
            }))
        }
    })).await {
        eprintln!("Server error: {}", e);
    }
    Ok(())
}
