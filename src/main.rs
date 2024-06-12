
#![no_std]
#![no_main]

use embassy_executor::Spawner;
use embassy_futures::join::join;
use embassy_net::{tcp::TcpSocket, Config, Ipv4Address, Ipv4Cidr, StaticConfigV4, Stack, StackResources};
use embassy_time::{with_timeout, Duration, Timer};
use esp_backtrace as _;
use esp_hal_embassy;
use esp_hal::{
    delay::Delay,
    clock::ClockControl,
    peripherals::Peripherals,
    prelude::*,
    rng::Rng,
    system::SystemControl,
    timer::timg::TimerGroup,
};
use esp_println::println;
use esp_wifi::{
    initialize, wifi::{
        ClientConfiguration,
        Configuration,
        WifiController,
        WifiDevice,
        WifiEvent,
        WifiStaDevice,
        WifiState,
    }, wifi_interface, EspWifiInitFor
};

use static_cell::StaticCell;
use heapless::Vec;
extern crate alloc;
use core::mem::MaybeUninit;
use alloc::borrow::BorrowMut;

#[global_allocator]
static ALLOCATOR: esp_alloc::EspHeap = esp_alloc::EspHeap::empty();

fn init_heap() {
    const HEAP_SIZE: usize = 32 * 1024;
    static mut HEAP: MaybeUninit<[u8; HEAP_SIZE]> = MaybeUninit::uninit();

    unsafe {
        ALLOCATOR.init(HEAP.as_mut_ptr() as *mut u8, HEAP_SIZE);
    }
}

const SSID: &str = env!("SSID");
const PASSWORD: &str = env!("PASSWORD");
// Local IP address
const HOST_IP: &str = env!("HOST_IP");

const TEST_DURATION: usize = 15;
const RX_BUFFER_SIZE: usize = 16384;
const TX_BUFFER_SIZE: usize = 16384;
const IO_BUFFER_SIZE: usize = 1024;
const PORT: u16 = 32580;

static mut RX_BUFFER: [u8; RX_BUFFER_SIZE] = [0; RX_BUFFER_SIZE];
static mut TX_BUFFER: [u8; TX_BUFFER_SIZE] = [0; TX_BUFFER_SIZE];

#[main]
async fn main(spawner: Spawner) -> ! {
    let peripherals = Peripherals::take();
    let system = SystemControl::new(peripherals.SYSTEM);

    let ip_address: Ipv4Address = HOST_IP.parse().expect("Invalid HOST_IP address");

    let clocks = ClockControl::max(system.clock_control).freeze();
    let delay = Delay::new(&clocks);
    init_heap();

    esp_println::logger::init_logger_from_env();

    let timer = esp_hal::timer::timg::TimerGroup::new(peripherals.TIMG1, &clocks, None).timer0;
    let init = esp_wifi::initialize(
        esp_wifi::EspWifiInitFor::Wifi,
        timer,
        esp_hal::rng::Rng::new(peripherals.RNG),
        peripherals.RADIO_CLK,
        &clocks,
    )
    .unwrap();

    let wifi = peripherals.WIFI;
    let (wifi_interface, controller) = 
        esp_wifi::wifi::new_with_mode(&init, wifi, WifiStaDevice).unwrap();

    let timer_group0 = TimerGroup::new_async(peripherals.TIMG0, &clocks);
    esp_hal_embassy::init(&clocks, timer_group0);

    // Set the IP addresses and DNS
    let config = Config::ipv4_static(StaticConfigV4 {
        address: Ipv4Cidr::new(ip_address, 24),
        gateway: Some(Ipv4Address::from_bytes(&[192, 168, 2, 1])),
        dns_servers: Default::default(),
    });
    let seed = 2727;
    static RESSOURCES : StaticCell<StackResources<4>> = StaticCell::<StackResources::<4>>::new();
    static STACK : StaticCell<Stack<WifiDevice<'_, WifiStaDevice>>> = StaticCell::new();
    
    let stack = STACK.uninit().write(
        Stack::new(
            wifi_interface,
            config,
            RESSOURCES.init(StackResources::<4>::new()),
            seed
        )
    );

    spawner.spawn(connection(controller)).ok();
    

    loop {
        log::info!("Hello world!");
        delay.delay(500.millis());
    }
}

// Wifi Connection task - Taken from esp-rs example
#[embassy_executor::task]
async fn connection(mut controller: WifiController<'static>) {
    println!("start connection task");
    println!("Device capabilities: {:?}", controller.get_capabilities());
    loop {
        match esp_wifi::wifi::get_wifi_state() {
            WifiState::StaConnected => {
                // wait until we're no longer connected
                controller.wait_for_event(WifiEvent::StaDisconnected).await;
                Timer::after(Duration::from_millis(5000)).await
            }
            _ => {}
        }
        if !matches!(controller.is_started(), Ok(true)) {
            let client_config = Configuration::Client(ClientConfiguration {
                ssid: SSID.try_into().unwrap(),
                password: PASSWORD.try_into().unwrap(),
                ..Default::default()
            });
            controller.set_configuration(&client_config).unwrap();
            println!("Starting wifi");
            controller.start().await.unwrap();
            println!("Wifi started!");
        }
        println!("About to connect...");

        match controller.connect().await {
            Ok(_) => println!("Wifi connected!"),
            Err(e) => {
                println!("Failed to connect to wifi: {e:?}");
                Timer::after(Duration::from_millis(5000)).await
            }
        }
    }
}
