fn is_local_host_fixed(value: &str) -> bool {
    let Ok(url) = url::Url::parse(&format!("https://{value}")) else {
        return false;
    };
    match url.host() {
        Some(url::Host::Ipv4(_) | url::Host::Ipv6(_)) => true,
        Some(url::Host::Domain(d)) => d.eq_ignore_ascii_case("localhost"),
        _ => false,
    }
}
fn main() {
    println!("[::1] -> {}", is_local_host_fixed("[::1]"));
    println!("[::1]:8080 -> {}", is_local_host_fixed("[::1]:8080"));
    println!("127.0.0.1:8080 -> {}", is_local_host_fixed("127.0.0.1:8080"));
}
