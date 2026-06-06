use std::path::{Path, PathBuf};
use std::sync::Arc;

use clap::{Parser, Subcommand};
use ryx_rs::cli::style::{cyan, fail_mark, green, magenta, ok_mark, prefix, red, warn_mark, yellow};
use ryx_rs::migration::ddl::DDLGenerator;
use ryx_rs::migration::files::{discover_migration_files, load_migration_file};
use ryx_rs::migration::runner::{operation_to_sql, FileRunner};
use ryx_rs::RyxConfig;

#[derive(Parser)]
#[command(name = "ryx", about = "Ryx ORM — command-line tool")]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    #[arg(long, global = true)]
    config: Option<String>,

    #[arg(long, global = true)]
    url: Option<String>,

    #[arg(long, global = true)]
    models: Option<String>,
}

#[derive(Subcommand)]
enum Commands {
    /// Apply pending migrations to the database
    Migrate {
        #[arg(long, default_value = "migrations")]
        dir: String,

        #[arg(long)]
        alias: Option<String>,

        #[arg(long)]
        dry_run: bool,

        #[arg(long)]
        no_interactive: bool,

        #[arg(long)]
        schema: Option<String>,
    },

    /// Detect model changes and generate migration files
    Makemigrations {
        #[arg(long, default_value = "migrations")]
        dir: String,

        #[arg(long)]
        check: bool,
    },

    /// List all migrations and their applied status
    Showmigrations {
        #[arg(long, default_value = "migrations")]
        dir: String,

        #[arg(long)]
        unapplied: bool,

        #[arg(long)]
        alias: Option<String>,
    },

    /// Print SQL for a migration without executing it
    Sqlmigrate {
        name: String,

        #[arg(long, default_value = "migrations")]
        dir: String,

        #[arg(long)]
        backends: Option<String>,

        #[arg(long)]
        schema: Option<String>,
    },
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    match &cli.command {
        Commands::Migrate { dir, alias, dry_run, no_interactive, schema } => {
            cmd_migrate(&cli, dir, alias.as_deref(), *dry_run, *no_interactive, schema.as_deref()).await;
        }
        Commands::Makemigrations { dir, check } => {
            cmd_makemigrations(dir, *check).await;
        }
        Commands::Showmigrations { dir, unapplied, alias } => {
            cmd_showmigrations(&cli, dir, *unapplied, alias.as_deref()).await;
        }
        Commands::Sqlmigrate { name, dir, backends, schema } => {
            cmd_sqlmigrate(dir, name, backends.as_deref(), schema.as_deref());
        }
    }
}

// ── migrate ─────────────────────────────────────────────

async fn cmd_migrate(
    cli: &Cli,
    dir: &str,
    alias: Option<&str>,
    dry_run: bool,
    no_interactive: bool,
    schema: Option<&str>,
) {
    let alias = alias.unwrap_or("default");
    init_pool(cli).await;

    let mut runner = FileRunner::new()
        .migrations_dir(dir)
        .db(alias)
        .dry_run(dry_run)
        .no_interactive(no_interactive);

    if let Some(s) = schema {
        runner = runner.schema(s);
    }

    let result = runner.run().await;

    match result {
        Ok(statements) => {
            if dry_run {
                println!();
                for s in &statements {
                    println!("{s}");
                }
                println!("{}  {} statement(s)", prefix(), green(&statements.len().to_string()));
            } else if statements.is_empty() {
                println!("{}  {}", prefix(), yellow("Nothing to migrate"));
            } else {
                println!("{}  {} {} applied", prefix(), ok_mark(), green(&statements.len().to_string()));
            }
        }
        Err(e) => {
            eprintln!("{}  {} {}", prefix(), fail_mark(), red(&e.to_string()));
            std::process::exit(1);
        }
    }
}

// ── makemigrations ──────────────────────────────────────

async fn cmd_makemigrations(_dir: &str, check: bool) {
    if check {
        let files = discover_migration_files(Path::new(_dir));
        if files.is_empty() {
            println!("{}  {}", prefix(), red("Unapplied changes — no migrations yet"));
            std::process::exit(1);
        }
        println!("{}  All migrations applied", prefix());
        return;
    }

    println!("{}  {} — use Python `ryx makemigrations` for now", prefix(), yellow("makemigrations"));
}

// ── showmigrations ──────────────────────────────────────

async fn cmd_showmigrations(cli: &Cli, dir: &str, unapplied: bool, alias: Option<&str>) {
    let alias = alias.unwrap_or("default");
    let files = discover_migration_files(Path::new(dir));

    if files.is_empty() {
        println!("{}  {}", prefix(), yellow("No migrations found"));
        return;
    }

    init_pool(cli).await;
    let applied = load_applied(alias).await;

    println!("\n{}  Migrations in {}:", prefix(), cyan(dir));
    for f in &files {
        let stem = f.file_stem().and_then(|s| s.to_str()).unwrap_or("?");
        let key = format!("{alias}|{stem}");
        let is_applied = applied.contains(&key) || applied.contains(&format!("{stem}"));
        if unapplied && is_applied {
            continue;
        }
        let status = if is_applied {
            format!("{} {}", ok_mark(), green(stem))
        } else {
            format!("  {}", yellow(stem))
        };
        println!("  [{status}]");
    }
    println!();
}

// ── sqlmigrate ──────────────────────────────────────────

fn cmd_sqlmigrate(dir: &str, name: &str, backends: Option<&str>, schema: Option<&str>) {
    let path = match find_migration_file(Path::new(dir), name) {
        Some(p) => p,
        None => {
            eprintln!("{}  {} Migration not found: {}", prefix(), fail_mark(), red(name));
            std::process::exit(1);
        }
    };

    let mf = match load_migration_file(&path) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("{}  {} {}", prefix(), fail_mark(), red(&e));
            std::process::exit(1);
        }
    };

    let backend_list: Vec<&str> = backends
        .map(|b| b.split(',').collect())
        .unwrap_or_else(|| vec!["postgres"]);

    for backend_name in &backend_list {
        let backend = match backend_name.trim() {
            "postgres" | "postgresql" => ryx_query::Backend::PostgreSQL,
            "mysql" => ryx_query::Backend::MySQL,
            "sqlite" => ryx_query::Backend::SQLite,
            _ => {
                eprintln!("{}  {} Unknown backend: {}", prefix(), warn_mark(), yellow(backend_name));
                continue;
            }
        };

        let ddl = DDLGenerator::new(backend).in_schema(schema.unwrap_or(""));
        let fname = path.file_name().and_then(|n| n.to_str()).unwrap_or("?");

        if backend_list.len() > 1 {
            println!("{}  SQL for {} [{}]", prefix(), cyan(fname), magenta(backend_name.trim()));
        } else {
            println!("{}  SQL for {}", prefix(), cyan(fname));
        }
        println!();

        for op in &mf.operations {
            for sql in operation_to_sql(&ddl, op) {
                println!("{sql}");
            }
        }
    }
}

// ── helpers ─────────────────────────────────────────────

fn load_config(cli: &Cli) -> Arc<RyxConfig> {
    let mut config = if let Some(ref path) = cli.config {
        RyxConfig::load_from_dir(path)
    } else {
        RyxConfig::load()
    };

    if let Some(ref url) = cli.url {
        config.urls.insert("default".into(), url.clone());
    }

    Arc::new(config)
}

async fn init_pool(cli: &Cli) {
    let config = load_config(cli);
    let _ = config.init_pool().await;
}

async fn load_applied(alias: &str) -> Vec<String> {
    let pool = match ryx_backend::pool::get(Some(alias)) {
        Ok(p) => p,
        Err(_) => return vec![],
    };

    let rows = match pool.fetch_raw("SELECT name FROM \"ryx_migrations\"".into(), None).await {
        Ok(r) => r,
        Err(_) => return vec![],
    };

    rows.iter()
        .filter_map(|r| {
            r.get("name").and_then(|v| match v {
                ryx_query::ast::SqlValue::Text(s) => Some(s.clone()),
                _ => None,
            })
        })
        .collect()
}

fn find_migration_file(dir: &Path, name: &str) -> Option<PathBuf> {
    let exact = dir.join(format!("{name}.yaml"));
    if exact.exists() {
        return Some(exact);
    }
    let exact_yml = dir.join(format!("{name}.yml"));
    if exact_yml.exists() {
        return Some(exact_yml);
    }
    let files = discover_migration_files(dir);
    files.into_iter().find(|f| {
        f.file_stem()
            .and_then(|s| s.to_str())
            .map(|s| s.starts_with(name))
            .unwrap_or(false)
    })
}
