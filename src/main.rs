use std::{fs::File, io::prelude::*, net::SocketAddr, path::PathBuf};

use anyhow::Error;
use chrono::{Duration, NaiveTime, Utc};
use chrono_tz::US::Pacific;
use structopt::StructOpt;

use sgip_ev_charging::{Config, Simulator, Validate};

#[derive(Debug, StructOpt)]
struct Opt {
    /// Command
    #[structopt(subcommand)]
    cmd: Command,
}

#[derive(Debug, StructOpt)]
enum Command {
    /// Generate a default configuration file.
    GenerateConfig {
        /// Output path for default config file
        #[structopt(short, long, parse(from_os_str))]
        output: PathBuf,
    },
    /// Run the charge controller using the provided config.
    Start {
        /// Config file path
        #[structopt(short, long, parse(from_os_str))]
        config: PathBuf,
        /// Prometheus endpoint address
        #[structopt(short, long)]
        prometheus_endpoint: Option<SocketAddr>,
    },
    /// Simulate the charging algorithm over historical data over a number of backtest days.
    Simulator {
        /// Config file path
        #[structopt(short, long, parse(from_os_str))]
        config: PathBuf,
        /// Number of days to backtest
        #[structopt(short, long)]
        backtest_days: usize,
        /// Prefix for output CSVs
        #[structopt(short, long)]
        prefix: String,
    },
    /// Merge the outputs of simulator runs into a single CSV.
    MergeCsv {
        /// Only emit state of charge data
        #[structopt(short, long)]
        soc_only: bool,
        /// Only emit marginal emissions data
        #[structopt(short, long)]
        emissions_only: bool,
        /// Output path for default config file
        #[structopt(short, long, parse(from_os_str))]
        output: PathBuf,
        /// Input files.
        #[structopt(parse(from_os_str))]
        inputs: Vec<PathBuf>,
    },
}

fn load_config(config: PathBuf) -> Config {
    let mut buf = String::new();
    File::open(config)
        .unwrap()
        .read_to_string(&mut buf)
        .unwrap();
    toml::from_str(&buf).unwrap()
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let opt = Opt::from_args();
    match opt.cmd {
        Command::Start {
            config,
            prometheus_endpoint,
        } => {
            let config = load_config(config);
            start(config, prometheus_endpoint).await.unwrap();
        }
        Command::GenerateConfig { output } => {
            let config_toml = toml::to_string_pretty(&Config::default()).unwrap();
            File::create(output)
                .unwrap()
                .write_all(config_toml.as_bytes())
                .unwrap();
        }
        Command::Simulator {
            config,
            backtest_days,
            prefix,
        } => {
            let config = load_config(config);
            simulator(config, backtest_days, prefix).await.unwrap();
        }
        Command::MergeCsv {
            output,
            inputs,
            soc_only,
            emissions_only,
        } => {
            merge_csv(output, inputs, soc_only, emissions_only).unwrap();
        }
    }
}

async fn start(config: Config, prometheus_endpoint: Option<SocketAddr>) -> Result<(), Error> {
    if let Some(addr) = prometheus_endpoint {
        metrics_exporter_prometheus::PrometheusBuilder::new()
            .listen_address(addr)
            .install()
            .unwrap();
    }

    let Config {
        charging,
        sgip_credentials,
        tesla_credentials,
    } = config;

    use sgip_ev_charging::tesla;
    use sgip_signal::SgipSignal;

    tracing::info!("Logging in to SGIP API");
    let sgip = SgipSignal::login(
        &sgip_credentials.sgip_username,
        &sgip_credentials.sgip_password,
    )
    .await?;

    tracing::info!("Logging in to Tesla API");
    let tesla_token = tesla::AccessToken::login(
        &tesla_credentials.tesla_username,
        &tesla_credentials.tesla_password,
        "sgip-ev-charging",
    )
    .await?;

    tracing::info!("Fetching vehicle info");
    let vehicle = tesla_token.vehicles("sgip-ev-charging").await?.remove(0);

    sgip_ev_charging::start(charging, sgip, vehicle).await
}

async fn simulator(config: Config, backtest_days: usize, prefix: String) -> Result<(), Error> {
    let config = config.validate().unwrap();

    for days_ago in 0..std::cmp::min(backtest_days, 21) {
        // Start at least 2 days ago to ensure data is available
        let start_day = (Utc::now() - Duration::days(4 + days_ago as i64))
            .with_timezone(&Pacific)
            .date();
        tracing::info!(?start_day, "Starting simulation run");

        let start_time = NaiveTime::from_hms(0, 0, 0);
        let start = start_day.and_time(start_time).unwrap().with_timezone(&Utc);
        let mut sim = Simulator::new(config.clone(), start);

        sim.run().await.unwrap();

        let output_path = format!("{}_{}.csv", prefix, start_day);
        tracing::info!(?output_path, "writing simulation data");

        let mut writer = csv::Writer::from_path(&output_path)?;
        for r in sim.take_records().into_iter() {
            writer.serialize(r).unwrap();
        }
    }

    Ok(())
}

fn merge_csv(
    output: PathBuf,
    inputs: Vec<PathBuf>,
    soc_only: bool,
    emissions_only: bool,
) -> Result<(), Error> {
    let mut writer = csv::Writer::from_path(&output)?;

    let mut readers = inputs
        .into_iter()
        .map(|input| {
            csv::ReaderBuilder::new()
                // setting this to false preserves the headers in processing
                .has_headers(false)
                .from_path(input)
                .map(|reader| reader.into_records())
        })
        .collect::<Result<Vec<_>, _>>()?;

    'merge: loop {
        let mut merged = Vec::new();
        for reader in readers.iter_mut() {
            let record = if let Some(record) = reader.next() {
                record?
            } else {
                break 'merge;
            };

            if merged.is_empty() {
                let time = record.get(1).expect("time should be present");
                merged.push(time.to_string());
            }

            if soc_only {
                merged.extend(record.iter().skip(2).take(4).map(String::from));
            } else if emissions_only {
                merged.extend(record.iter().skip(6).take(1 + 4 + 4).map(String::from));
            } else {
                merged.extend(record.iter().skip(2).map(String::from));
            }
        }
        writer.write_record(merged)?;
    }

    Ok(())
}
