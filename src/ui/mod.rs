pub mod server;

use std::path::Path;

use server::UiServer;

use crate::error::GraphiaError;

pub fn open_browser_window(url: &str) {
    #[cfg(target_os = "windows")]
    {
        let _ = std::process::Command::new("cmd")
            .args(["/c", "start", url])
            .spawn();
    }
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg(url).spawn();
    }
    #[cfg(target_os = "linux")]
    {
        let _ = std::process::Command::new("xdg-open").arg(url).spawn();
    }
}

pub fn run_ui(repo_path: &Path, port: u16, open_browser: bool) -> Result<(), GraphiaError> {
    let graph = if repo_path.join(".graphia/index.bin").exists() {
        crate::storage::load_graph_binary(&repo_path.join(".graphia/index.bin"))?
    } else if repo_path.join("graph.json").exists() {
        crate::storage::load_graph_json(&repo_path.join("graph.json"))?
    } else {
        println!("Index not found. Building graph from repository...");
        crate::storage::build_graph_from_repo(repo_path)?
    };

    let server = UiServer::new(graph, repo_path.to_path_buf(), port);
    let bound_port = server.start().map_err(|e| GraphiaError::Io {
        path: repo_path.to_path_buf(),
        message: e.to_string(),
    })?;

    let url = format!("http://127.0.0.1:{bound_port}");

    println!("============================================================");
    println!("  Graphia Interactive Browser Explorer");
    println!("  URL: {url}");
    println!("  Press Ctrl+C to stop server");
    println!("============================================================");

    if open_browser {
        open_browser_window(&url);
    }

    let (tx, rx) = std::sync::mpsc::channel();
    let _ = ctrlc::set_handler(move || {
        let _ = tx.send(());
    });

    // Wait until Ctrl+C is pressed
    let _ = rx.recv();
    println!("\nShutting down Graphia UI server...");
    server.stop();

    Ok(())
}
