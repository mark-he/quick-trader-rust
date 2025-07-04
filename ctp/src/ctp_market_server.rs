#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]
use common::error::AppError;
use market::kline::KLineCombiner;
use market::sim_market_server::KLineLoader;
use ureq::{Agent, AgentBuilder, Response};
use crate::model::{self, CtpConfig, Symbol};

use super::ctp_market_cpi::Spi;
use market::market_server::{KLine, MarketData, MarketServer, Tick};
use libctp_sys::*;
use std::ffi::{CStr, CString};
use std::os::raw::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use common::msmc::*;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

struct SafePointer<T>(*mut T);

unsafe impl<T> Send for SafePointer<T> {}


#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ApiConfig {
    flow_path: String,
    is_udp: bool,
    is_multicast: bool,
    front_addr: Vec<String>,
}

pub struct MDApi {
    api: Rust_CThostFtdcMdApi,
    spi: Option<SafePointer<Rust_CThostFtdcMdSpi>>,
    config: ApiConfig,
}


impl MDApi {
    pub fn get_version() -> String {
        let cs = unsafe { CStr::from_ptr(CThostFtdcMdApi::GetApiVersion()) };
        cs.to_string_lossy().into()
    }

    pub fn new(config: ApiConfig) -> Self {
        let cs = std::ffi::CString::new(config.flow_path.as_bytes()).unwrap();
        let api = unsafe {
            Rust_CThostFtdcMdApi::new(CThostFtdcMdApi::CreateFtdcMdApi(
                cs.as_ptr(),
                config.is_udp,
                config.is_multicast,
            ))
        };
        Self {
            api,
            spi: None,
            config: config.clone(),
        }
    }

    fn req_init(&mut self) -> Subscription<MarketData> {
        let mut top = Subscription::top();
        let outer_subscription = top.subscribe();

        self.register(Spi::new(top));

        for addr in &self.config.front_addr {
            let cs = CString::new(addr.as_bytes()).unwrap();
            unsafe {
                self.api.RegisterFront(cs.as_ptr() as *mut _);
            }
        }
        unsafe {
            self.api.Init();
        }
        outer_subscription
    }

    fn req_user_login(&mut self) -> Result<(), String> {
        let mut loginfield = CThostFtdcReqUserLoginField {
            TradingDay: Default::default(),
            BrokerID: Default::default(),
            UserID: Default::default(),
            Password: [0i8; 41],
            UserProductInfo: Default::default(),
            InterfaceProductInfo: Default::default(),
            ProtocolInfo: Default::default(),
            MacAddress: Default::default(),
            OneTimePassword: [0i8; 41],
            ClientIPAddress: [0; 33],
            LoginRemark: [0i8; 36],
            ClientIPPort: Default::default(),
            reserve1: [0; 16],
        };

        unsafe {
            self.api.ReqUserLogin(&mut loginfield, 1);
        }
        Ok(())
    }

    fn check_connected(&mut self, subscription: &Subscription<MarketData>) -> Result<(), String> {
        let mut should_break = false;
        loop {
            let ret = subscription.recv_timeout(5,  &mut |event| {
                match event {
                    MarketData::Connected => {
                        should_break = true;
                    },
                    _ => {}
                }
            });
            if ret.is_err() {
                return Err(format!("Error happened when connecting to market server: {:?}", ret.unwrap_err()));
            }
            if should_break {
                break;
            }
        }
        Ok(())
    }

    fn check_logined(&mut self, subscription: &Subscription<MarketData>) -> Result<(), String> {
        let mut should_break = false;
        loop {
            let ret = subscription.recv_timeout(5,  &mut |event| {
                match event {
                    MarketData::UserLogin => {
                        should_break = true;
                    },
                    _ => {}
                }
            });
            if ret.is_err() {
                return Err(format!("Error happened when logining to market server: {:?}", ret.unwrap_err()));
            }
            if should_break {
                break;
            }
        }
        Ok(())
    }

    pub fn start(&mut self) -> Result<Subscription<MarketData>, String> {
        let subscription = self.req_init();
        let ret = self.check_connected(&subscription);
        if ret.is_err() {
            return Err(ret.unwrap_err());
        }

        self.req_user_login()?;
        let ret = self.check_logined(&subscription);
        if ret.is_err() {
            return Err(ret.unwrap_err());
        }
        Ok(subscription)
    }

    pub fn subscribe_market_data(&mut self, codes: &[&str], is_unsub: bool) -> Result<(), String> {
        let len = codes.len() as c_int;
        let arr_cstring: Vec<CString> = codes
            .iter()
            .map(|s| CString::new(s.as_bytes()).unwrap())
            .collect();
        let arr_cstr: Vec<*mut c_char> = arr_cstring
            .iter()
            .map(|s| s.as_ptr() as *mut c_char)
            .collect();
        let ptr = arr_cstr.as_ptr() as *mut *mut c_char;
        let rtn = if is_unsub {
            unsafe { self.api.UnSubscribeMarketData(ptr, len) }
        } else {
            unsafe { self.api.SubscribeMarketData(ptr, len) }
        };
        if rtn != 0 {
            return Err(format!(
                "Fail to req `md_api_subscribe_market_data`: {}",
                rtn
            ));
        }
        Ok(())
    }

    fn register<S: Rust_CThostFtdcMdSpi_Trait>(&mut self, spi: S) {
        if let Some(spi) = self.spi.take() {
            Self::drop_spi(spi);
        }

        let spi: Box<Box<dyn Rust_CThostFtdcMdSpi_Trait>> = Box::new(Box::new(spi));
        let ptr = Box::into_raw(spi) as *mut _ as *mut c_void;

        let spi_stub = unsafe { Rust_CThostFtdcMdSpi::new(ptr) };
        let spi: *mut Rust_CThostFtdcMdSpi = Box::into_raw(Box::new(spi_stub));
        unsafe {
            self.api.RegisterSpi(spi as _);
        }

        self.spi = Some(SafePointer(spi));
    }

    fn drop_spi(spi: SafePointer<Rust_CThostFtdcMdSpi>) {
        let mut spi = unsafe { Box::from_raw(spi.0) };
        unsafe {
            spi.destruct();
        }
    }
}

impl Drop for MDApi {
    fn drop(&mut self) {
        unsafe {
            self.api.destruct();
        }
        if let Some(spi) = self.spi.take() {
            Self::drop_spi(spi);
        }
    }
}

#[derive(Clone, Debug)]
pub struct MarketTopic {
    pub symbol: String,
    pub interval: String,
}

pub struct CtpKlineLoader {
    client: Agent,
    base_url: String,
}

impl CtpKlineLoader {
    pub fn new(base_url: &str) -> Self {
        CtpKlineLoader {
            client: AgentBuilder::new().build(),
            base_url: base_url.to_string(),
        }
    }

    fn send(&self, method: &str, path: &str, params: HashMap<String, String>)  -> Result<Response, AppError> {
        let url = format!("{}/{}", self.base_url, path);
        let mut ureq_request = self.client.request(method.as_ref(), &url);
        ureq_request = ureq_request.set("User-Agent", "ctp-connector");

        let has_params = !params.is_empty();
        if has_params {
            for (k, v) in params.iter() {
                ureq_request = ureq_request.query(k, v);
            }
        }
        let response = ureq_request.call().map_err(|e| AppError::new(-200, &e.to_string()))?;
        Ok(response)
    }
}

impl KLineLoader for CtpKlineLoader {
    fn load_kline(&self, symbol: &str, interval: &str, count: u32, start_time: Option<u64>, end_time: Option<u64>) -> Result<Vec<KLine>, AppError> {
        let mut params: HashMap<String, String> = HashMap::new();
        params.insert("symbol".to_string(), symbol.to_string());
        params.insert("interval".to_string(), interval.to_string());
        let path;

        if start_time.is_some() {
            params.insert("start".to_string(), start_time.unwrap().to_string());
            path = "/ctp/klines";
        } else {
            if end_time.is_some() {
                params.insert("end".to_string(), end_time.unwrap().to_string());
            }
            params.insert("limit".to_string(), count.to_string());
            path = "/ctp/klines/n";
        }
        let ret = self.send("GET", path, params);
        let klines = model::get_resp_result::<Vec<KLine>>(ret, false)?;
        Ok(klines.unwrap())
    }
}

pub struct CtpMarketServer {
    mapi: Option<MDApi>,
    topics: Vec<MarketTopic>,
    config: CtpConfig,
    handler: Option<JoinHandle<()>>,
    start_ticket: Arc<AtomicUsize>,
    subscription: Arc<Mutex<Subscription<MarketData>>>,
}

impl CtpMarketServer {
    pub fn new(config: CtpConfig) -> Self {
        CtpMarketServer {
            mapi: None,
            config,
            topics: Vec::new(),
            handler: None,
            start_ticket: Arc::new(AtomicUsize::new(0)),
            subscription: Arc::new(Mutex::new(Subscription::top())),
        }
    }
}

impl MarketServer for CtpMarketServer {
    type Symbol = Symbol;
    fn init(&mut self) -> Result<(), AppError> {
        Ok(())
    }

    fn start(&mut self) -> Result<Subscription<MarketData>, AppError> {
        let start_ticket = self.start_ticket.fetch_add(1, Ordering::SeqCst);
        let start_ticket_ref = self.start_ticket.clone();
        let mut mapi = MDApi::new(ApiConfig {
            flow_path: "".into(),
            front_addr: vec![format!("tcp://{}", self.config.nm_addr.clone())],
            ..Default::default()
        });
        let mut subscription = mapi.start().unwrap();
        subscription.name = "CTP MARKETSERVER".to_string();
        let outer_subscription = subscription.subscribe();
        self.subscription = Arc::new(Mutex::new(subscription));
        
        let mut tick_set = HashSet::new();
        for topic in self.topics.iter() {
            if !tick_set.contains(topic.symbol.as_str()) {
                mapi.subscribe_market_data(&[topic.symbol.as_str()], false).unwrap();
                tick_set.insert(topic.symbol.to_string());
            }
        }
        self.mapi = Some(mapi);

        let topics = self.topics.clone();
        let mut last_ticks = HashMap::<String, Tick>::new();
        let mut combiner_map:HashMap<String, KLineCombiner> = HashMap::new();


        let subscription_ref = self.subscription.clone();
        let handler = self.subscription.lock().unwrap().stream(move |event| {
            if start_ticket != start_ticket_ref.load(Ordering::SeqCst) - 1 {
                return Err(StreamError::Exit);
            }
            match event {
                Some(data) => {
                    match data {
                        MarketData::Tick(t) => {
                            let subscription = subscription_ref.lock().unwrap();
                            subscription.send(&MarketData::Tick(t.clone()));
                            
                            let mut volumn = 0 as f64;
                            let mut turnover = 0 as f64;
                            let prev_tick = last_ticks.get(&t.symbol);
                            if let Some(prev) = prev_tick {
                                volumn = t.volume - prev.volume;
                                turnover = t.turnover - prev.turnover;
                            }
                            last_ticks.insert(t.symbol.to_string(), t.clone());

                            for topic in topics.iter() {
                                if topic.symbol == t.symbol && topic.interval != "" {
                                    let combiner = combiner_map.entry(format!("{}_{}", topic.symbol, topic.interval)).or_insert(KLineCombiner::new(topic.interval.as_str(), 100, Some(21)));
                                    let kline = KLine {
                                        symbol: t.symbol.clone(),
                                        datetime: t.datetime.clone(),
                                        interval: topic.interval.clone(),
                                        open: t.close,
                                        high: t.close,
                                        low: t.close,
                                        close: t.close,
                                        volume: volumn,
                                        turnover: turnover,
                                        //taker_buy_volume: 0.0,
                                        //taker_buy_turnover: 0.0,
                                        timestamp: t.timestamp,
                                    };
                                    let mut new_kline = combiner.combine_tick(&kline, true);
                                    if let Some(kline) = new_kline.take() {
                                        let _ = subscription.send(&MarketData::Kline(kline));
                                    }
                                }
                            }
                            return Ok(false);
                        },
                        _ => {
                        },
                    }
                },
                None => {
                }
            }
            Ok(true)
        });
        self.handler = Some(handler);
        Ok(outer_subscription)
    }
    
    fn load_kline(&mut self, symbol: Symbol, interval: &str, count: u32) -> Result<Vec<KLine>, AppError> {
        let symbol_str = format!("{}.{}", symbol.exchange_id.as_str(), symbol.symbol.as_str());
        let klines = CtpKlineLoader::new("http://127.0.0.1:5001").load_kline(&symbol_str, interval, count, None, None)?;
        Ok(klines)
    }

    fn get_server_ping(&self) -> usize {
        0
    }

    fn close(&self) {
        self.start_ticket.fetch_add(1, Ordering::SeqCst);
    }

    fn subscribe_tick(&mut self, symbol: Symbol) -> Result<(), AppError> {
        let mut found = false;
        for topic in self.topics.iter() {
            if topic.symbol == symbol.symbol {
                found = true;
                break;
            }
        }
        if !found {
            let topic = MarketTopic {
                symbol: symbol.symbol.clone(),
                interval: "".to_string(),
            };
            self.topics.push(topic);
        }
        Ok(())
    }

    fn subscribe_kline(&mut self, symbol: Symbol, interval: &str) -> Result<(), AppError> {
        let mut found = false;
        for topic in self.topics.iter() {
            if topic.symbol == symbol.symbol && topic.interval == interval {
                
                found = true;
                break;
            }
        }
        if !found {
            let topic = MarketTopic {
                symbol: symbol.symbol.clone(),
                interval: interval.to_string(),
            };
            self.topics.push(topic);
        }
        Ok(())
    }
}