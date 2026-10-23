/// Parseia `"00:03:29.04"` (formato do progresso do ffmpeg-sidecar) pra
/// segundos. `0.0` se não reconhecer o formato — não é crítico, só afeta a
/// barra de progresso.
pub fn parse_time(raw: &str) -> f64 {
    let mut parts = raw.split(':');
    let (Some(h), Some(m), Some(s)) = (parts.next(), parts.next(), parts.next()) else {
        return 0.0;
    };
    let h: f64 = h.parse().unwrap_or(0.0);
    let m: f64 = m.parse().unwrap_or(0.0);
    let s: f64 = s.parse().unwrap_or(0.0);
    h * 3600.0 + m * 60.0 + s
}
