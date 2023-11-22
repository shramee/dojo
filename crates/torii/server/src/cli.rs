mod proxy;

use std::collections::HashMap;
use std::net::SocketAddr;
use std::str::FromStr;
use std::sync::Arc;

use camino::Utf8PathBuf;
use clap::Parser;
use dojo_types::primitive::Primitive;
use dojo_types::schema::Ty;
use dojo_world::contracts::world::WorldContractReader;
use dojo_world::manifest::{abi, ComputedValueEntrypoint, Manifest};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::SqlitePool;
use starknet::core::types::FieldElement;
use starknet::core::utils::get_selector_from_name;
use starknet::providers::jsonrpc::HttpTransport;
use starknet::providers::JsonRpcClient;
use tokio::sync::broadcast;
use tokio::sync::broadcast::Sender;
use tokio_stream::StreamExt;
use torii_core::engine::{Engine, EngineConfig, Processors};
use torii_core::processors::metadata_update::MetadataUpdateProcessor;
use torii_core::processors::register_model::RegisterModelProcessor;
use torii_core::processors::store_set_record::StoreSetRecordProcessor;
use torii_core::processors::store_transaction::StoreTransactionProcessor;
use torii_core::simple_broker::SimpleBroker;
use torii_core::sql::Sql;
use torii_core::types::{ComputedValueCall, Model};
use tracing::{error, info};
use tracing_subscriber::{fmt, EnvFilter};
use url::Url;

use crate::proxy::Proxy;

/// Dojo World Indexer
#[derive(Parser, Debug)]
#[command(name = "torii", author, version, about, long_about = None)]
struct Args {
    /// The world to index
    #[arg(short, long = "world", env = "DOJO_WORLD_ADDRESS")]
    world_address: FieldElement,

    /// Path to build manifest.json file.
    #[arg(short, long, env = "DOJO_MANIFEST_PATH")]
    pub manifest_json: Option<Utf8PathBuf>,

    /// The rpc endpoint to use
    #[arg(long, default_value = "http://localhost:5050")]
    rpc: String,
    /// Database filepath (ex: indexer.db). If specified file doesn't exist, it will be
    /// created. Defaults to in-memory database
    #[arg(short, long, default_value = ":memory:")]
    database: String,
    /// Specify a block to start indexing from, ignored if stored head exists
    #[arg(short, long, default_value = "0")]
    start_block: u64,
    /// Host address for api endpoints
    #[arg(long, default_value = "0.0.0.0")]
    host: String,
    /// Port number for api endpoints
    #[arg(long, default_value = "8080")]
    port: u16,
    /// Specify allowed origins for api endpoints (comma-separated list of allowed origins, or "*"
    /// for all)
    #[arg(long, default_value = "*")]
    #[arg(value_delimiter = ',')]
    allowed_origins: Vec<String>,
    /// The external url of the server, used for configuring the GraphQL Playground in a hosted
    /// environment
    #[arg(long)]
    external_url: Option<Url>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let filter_layer = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,hyper_reverse_proxy=off"));

    let subscriber = fmt::Subscriber::builder().with_env_filter(filter_layer).finish();

    // Set the global subscriber
    tracing::subscriber::set_global_default(subscriber)
        .expect("Failed to set the global tracing subscriber");

    // Setup cancellation for graceful shutdown
    let (shutdown_tx, _) = broadcast::channel(1);

    let shutdown_tx_clone = shutdown_tx.clone();
    ctrlc::set_handler(move || {
        let _ = shutdown_tx_clone.send(());
    })
    .expect("Error setting Ctrl-C handler");

    let database_url = format!("sqlite:{}", &args.database);
    let options = SqliteConnectOptions::from_str(&database_url)?.create_if_missing(true);
    let pool = SqlitePoolOptions::new()
        .min_connections(1)
        .max_connections(5)
        .connect_with(options)
        .await?;

    sqlx::migrate!("../migrations").run(&pool).await?;

    let provider: Arc<_> = JsonRpcClient::new(HttpTransport::new(Url::parse(&args.rpc)?)).into();

    // Get world address
    let world = WorldContractReader::new(args.world_address, &provider);

    let computed_values = computed_value_entrypoints(args.manifest_json);

    println!("{:#?}", computed_values);

    let mut db = Sql::new(pool.clone(), args.world_address).await?;
    let processors = Processors {
        event: vec![
            Box::new(RegisterModelProcessor { computed_values }),
            Box::new(StoreSetRecordProcessor),
            Box::new(MetadataUpdateProcessor),
        ],
        transaction: vec![Box::new(StoreTransactionProcessor)],
        ..Processors::default()
    };

    let (block_tx, block_rx) = tokio::sync::mpsc::channel(100);

    let mut engine = Engine::new(
        world,
        &mut db,
        &provider,
        processors,
        EngineConfig { start_block: args.start_block, ..Default::default() },
        shutdown_tx.clone(),
        Some(block_tx),
    );

    let addr: SocketAddr = format!("{}:{}", args.host, args.port).parse()?;

    let shutdown_rx = shutdown_tx.subscribe();
    let (grpc_addr, grpc_server) = torii_grpc::server::new(
        shutdown_rx,
        &pool,
        block_rx,
        args.world_address,
        Arc::clone(&provider),
    )
    .await?;

    let proxy_server = Arc::new(Proxy::new(addr, args.allowed_origins, Some(grpc_addr), None));

    let graphql_server = spawn_rebuilding_graphql_server(
        shutdown_tx.clone(),
        pool.into(),
        args.external_url,
        proxy_server.clone(),
    );

    info!("🚀 Torii listening at {}", format!("http://{}", addr));
    info!("Graphql playground: {}\n", format!("http://{}/graphql", addr));

    tokio::select! {
        _ = engine.start() => {},
        _ = proxy_server.start(shutdown_tx.subscribe()) => {},
        _ = graphql_server => {},
        _ = grpc_server => {},
    };

    Ok(())
}

async fn spawn_rebuilding_graphql_server(
    shutdown_tx: Sender<()>,
    pool: Arc<SqlitePool>,
    external_url: Option<Url>,
    proxy_server: Arc<Proxy>,
) {
    let mut broker = SimpleBroker::<Model>::subscribe();

    loop {
        let shutdown_rx = shutdown_tx.subscribe();
        let (new_addr, new_server) =
            torii_graphql::server::new(shutdown_rx, &pool, external_url.clone()).await;

        tokio::spawn(new_server);

        proxy_server.set_graphql_addr(new_addr).await;

        // Break the loop if there are no more events
        if broker.next().await.is_none() {
            break;
        }
    }
}

fn function_return_type_from_abi(
    computed_val_fn: &ComputedValueEntrypoint,
    abi: &Option<abi::Contract>,
) -> Ty {
    match abi {
        Some(abi) => abi
            .clone()
            .into_iter()
            .find_map(|i| {
                if let abi::Item::Function(fn_item) = i {
                    if fn_item.name != computed_val_fn.entrypoint {
                        return None;
                    }

                    Some(match fn_item.outputs[0].ty.as_str() {
                        "core::integer::u8" => Ty::Primitive(Primitive::U8(None)),
                        "core::integer::u16" => Ty::Primitive(Primitive::U16(None)),
                        "core::integer::u32" => Ty::Primitive(Primitive::U32(None)),
                        "core::integer::u64" => Ty::Primitive(Primitive::U64(None)),
                        "core::integer::u128" => Ty::Primitive(Primitive::U128(None)),
                        "core::integer::u256" => Ty::Primitive(Primitive::U256(None)),
                        "core::bool" => Ty::Primitive(Primitive::Bool(None)),
                        "core::felt252" => Ty::Primitive(Primitive::Felt252(None)),
                        "core::starknet::class_hash::ClassHash" => {
                            Ty::Primitive(Primitive::ClassHash(None))
                        }
                        "core::starknet::contract_address::ContractAddress" => {
                            Ty::Primitive(Primitive::ContractAddress(None))
                        }
                        ty => {
                            panic!(
                                "Unsupported computed value type {ty}. Only primitives \
                                 supported.\n{:?}",
                                computed_val_fn
                            )
                        }
                    })
                } else {
                    None
                }
            })
            .unwrap(),
        None => {
            error!("Error, Contract ABI not found.");
            Ty::Tuple(vec![])
        }
    }
}

fn computed_value_entrypoints(
    manifest_json: Option<Utf8PathBuf>,
) -> HashMap<String, Vec<ComputedValueCall>> {
    let mut computed_values: HashMap<String, Vec<ComputedValueCall>> = HashMap::new();
    if let Some(manifest) = manifest_json {
        match Manifest::load_from_path(manifest) {
            Ok(manifest) => {
                manifest.contracts.iter().for_each(|contract| {
                    contract.computed.iter().for_each(|computed_val_fn| {
                        if let Some(model_name) = computed_val_fn.model.clone() {
                            let return_type =
                                function_return_type_from_abi(computed_val_fn, &contract.abi);
                            let contract_entrypoint = ComputedValueCall {
                                contract_address: contract.address.unwrap(),
                                entry_point_selector: get_selector_from_name(
                                    &computed_val_fn.entrypoint.to_string(),
                                )
                                .unwrap(),
                                return_type,
                            };
                            match computed_values.get_mut(&model_name) {
                                Some(model_computed_values) => {
                                    model_computed_values.push(contract_entrypoint);
                                }
                                None => {
                                    computed_values.insert(model_name, vec![contract_entrypoint]);
                                }
                            };
                        }
                    })
                });
            }
            Err(err) => {
                error!("Manifest error: \n{:?}", err);
            }
        }
        // model
    };
    computed_values
}
