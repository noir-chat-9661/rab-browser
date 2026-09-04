fn is_local_host(value: &str) -> bool {
    let Ok(url) = url::Url::parse(&format!("https://{value}")) else {
        return false;
    };
    let Some(host) = url.host_str() else {
        return false;
    };
    host.eq_ignore_ascii_case("localhost") || host.parse::<std::net::IpAddr>().is_ok()
}

fn is_likely_domain(value: &str) -> bool {
    if value.chars().any(char::is_whitespace) {
        return false;
    }
    let Ok(url) = url::Url::parse(&format!("https://{value}")) else {
        return false;
    };
    let Some(host) = url.host_str() else {
        return false;
    };
    if host.eq_ignore_ascii_case("localhost") || host.parse::<std::net::IpAddr>().is_ok() {
        return true;
    }
    let Some((domain, tld)) = host.rsplit_once('.') else {
        return false;
    };
    !domain.is_empty() && tld.len() >= 2
}

fn normalize_url(value: &str) -> String {
    if is_likely_domain(value) {
        let scheme = if is_local_host(value) { "http" } else { "https" };
        format!("{scheme}://{value}")
    } else {
        format!("SEARCH: {}", value)
    }
}

fn main() {
    println!("::1 -> {}", normalize_url("::1"));
    println!("[::1] -> {}", normalize_url("[::1]"));
    println!("[::1]:8080 -> {}", normalize_url("[::1]:8080"));
}
