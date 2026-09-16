const NGINX: &str = include_str!("../../deploy/nginx.conf");
const FAUCETS: [(&str, &str); 2] = [
    ("faucet.seismictest.net", "http://localhost:3000"),
    ("community-faucet.seismictest.net", "http://localhost:3001"),
];

#[test]
fn each_faucet_has_its_own_https_upstream() {
    for (hostname, upstream) in FAUCETS {
        let servers: Vec<_> = NGINX
            .split("server {")
            .filter(|server| server.contains(&format!("server_name {hostname};")))
            .collect();
        let https: Vec<_> = servers
            .iter()
            .filter(|server| server.contains("listen 443 ssl;"))
            .collect();
        assert_eq!(
            https.len(),
            1,
            "missing or duplicate HTTPS host: {hostname}"
        );
        assert!(
            https[0].contains(&format!("location / {{\n        proxy_pass {upstream};")),
            "wrong frontend for {hostname}"
        );
        let http = servers
            .iter()
            .find(|server| server.contains("listen 80;"))
            .expect("HTTP listener must redirect to HTTPS");
        assert!(http.contains("return 301 https://$host$request_uri;"));
        assert!(!http.contains("proxy_pass"));
    }
}
