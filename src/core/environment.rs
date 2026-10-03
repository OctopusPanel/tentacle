use std::collections::HashMap;
use regex::Regex;

pub struct EnvironmentInterpolator;

impl EnvironmentInterpolator {
    pub fn interpolate(template: &str, vars: &HashMap<String, String>) -> String {
        let double_brace = Regex::new(r"\{\{([A-Za-z0-9_]+)\}\}").expect("valid regex");
        let result = double_brace.replace_all(template, |caps: &regex::Captures| {
            let key = &caps[1];
            vars.get(key).cloned().unwrap_or_else(|| caps[0].to_string())
        });

        let dollar_brace = Regex::new(r"\$\{([A-Za-z0-9_]+)\}").expect("valid regex");
        let result = dollar_brace.replace_all(&result, |caps: &regex::Captures| {
            let key = &caps[1];
            vars.get(key).cloned().unwrap_or_else(|| caps[0].to_string())
        });

        result.into_owned()
    }

    pub fn to_docker_env(vars: &HashMap<String, String>) -> Vec<String> {
        vars.iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect()
    }
}
