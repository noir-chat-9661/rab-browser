fn main() {
    let url = url::Url::parse("https://[::1]:8080").unwrap();
    println!("host: {:?}", url.host());
}
