fn main() {
    println!("Tentacle Daemon Starting...");
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_daemon_starts() {
        assert_eq!(2 + 2, 4);
    }
}
