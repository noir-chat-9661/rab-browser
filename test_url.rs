fn main() {
    let url = url::Url::parse("https://[::1]:8080").unwrap();
    println!("host_str: {:?}", url.host_str());
    println!("ip parse: {:?}", url.host_str().unwrap().parse::<std::net::IpAddr>());
}
