#![no_std]
#![no_main]
#![allow(unused_imports, dead_code, unused_variables, unused_mut)]

use core::panic::PanicInfo;
use embassy_executor::Spawner;

// GPIO
use embassy_rp::gpio::{Output, Level, Input, Pull, Pin};
use embassy_rp::peripherals::{ADC, I2C0, USB, PWM_SLICE1, PWM_SLICE2, PWM_SLICE3, PIN_0, PIN_1, PIN_2, PIN_3, PIN_4, PIN_6, PIN_18, PIN_19, PIN_20, PIN_21, PIN_26};

// USB
use embassy_rp::usb::{Driver, InterruptHandler};
use embassy_rp::{bind_interrupts};
use log::info;

// SPI
use embassy_rp::spi::{Spi, Config as SpiConfig};

// SDCard
use heapless::Vec;
use heapless::String;
use embedded_sdmmc::*;
use core::fmt::Write; // Import the Write trait

// Channel
use embassy_sync::channel::{Channel, Sender};
use embassy_sync::blocking_mutex::raw::ThreadModeRawMutex;

// Timer
use embassy_time::{Timer, Duration};

// ADC
use embassy_rp::adc::{
    Adc, Async, Channel as AdcChannel, Config as AdcConfig, InterruptHandler as InterruptHandlerAdc,
};

// PWM
use embassy_rp::pwm::{Config as PwmConfig, Pwm};

// Display
use embassy_rp::i2c::{Blocking, Config as I2cConfig, I2c};
use embedded_graphics::mono_font::iso_8859_16::FONT_10X20;
use embedded_graphics::mono_font::iso_8859_16::FONT_6X12;
use embedded_graphics::mono_font::MonoTextStyle;
use embedded_graphics::pixelcolor::BinaryColor;
use embedded_graphics::prelude::*;
use embedded_graphics::text::Text;
use ssd1306::{prelude::*, I2CDisplayInterface, Ssd1306, mode::BufferedGraphicsMode, mode::DisplayConfig};

const DISPLAY_FREQ: u32 = 400_000;

bind_interrupts!(struct Irqs {
    USBCTRL_IRQ => InterruptHandler<USB>;
    ADC_IRQ_FIFO => InterruptHandlerAdc;
});

// RGB Led colors
enum LedColor {
    Red,
    Yellow,
    Blue,
}
static TOP: u16 = 0x8000;

// Declare the channel as static
static CHANNEL: Channel<ThreadModeRawMutex, u16, 64> = Channel::new();

#[embassy_executor::task]
async fn logger_task(driver: Driver<'static, USB>) {
    embassy_usb_logger::run!(1024, log::LevelFilter::Info, driver);
}

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    // Peripherals initialization
    let peripherals = embassy_rp::init(Default::default());

    // SDCard initialization
    let mut sdconfig = SpiConfig::default(); // Initialize sdconfig
    sdconfig.frequency = 400_000;

    let mut miso = peripherals.PIN_12;
    let mut mosi = peripherals.PIN_11;
    let mut clk = peripherals.PIN_10;
    let mut cs = Output::new(peripherals.PIN_13, Level::High);

    let mut spi = Spi::new(
        peripherals.SPI1,
        clk,
        mosi,
        miso,
        peripherals.DMA_CH0,
        peripherals.DMA_CH1,
        sdconfig.clone(),
    );

    //I don't know why, but the SDCard doesn't work with this... Other guys on internet say it's ok, other colleagues says the same but it didn't work for me.
    // let mut controller = Controller::new(SdMmcSpi::new(spi, cs), VolumeIdx(0));
    // let volume = controller.get_volume(VolumeIdx(0)).unwrap();
    // let root_dir = controller.open_root_dir(&volume).unwrap();

    // The USB driver
    let driver = Driver::new(peripherals.USB, Irqs);
    // Spawn the logger task
    spawner.spawn(logger_task(driver)).unwrap();

    // Initialize buttons for play, pause, previous, and next
    let mut button_play = Input::new(peripherals.PIN_18, Pull::Up);
    let mut button_pause = Input::new(peripherals.PIN_19, Pull::Up);
    let mut button_previous = Input::new(peripherals.PIN_20, Pull::Up);
    let mut button_next = Input::new(peripherals.PIN_21, Pull::Up);

    // RGB Led colors initialization
    // Create configuration for red LED
    let mut config_red: PwmConfig = Default::default();
    config_red.top = TOP;
    config_red.compare_b = config_red.top;

    // Create configuration for green LED
    let mut config_green: PwmConfig = Default::default();
    config_green.top = TOP;
    config_green.compare_b = 0;

    // Create configuration for blue LED
    let mut config_blue: PwmConfig = Default::default();
    config_blue.top = TOP;
    config_blue.compare_b = 0;

    // Initialize PWM for red LED
    let mut pwm_red = Pwm::new_output_b(peripherals.PWM_SLICE1, peripherals.PIN_3, config_red.clone());
    // Initialize PWM for green LED
    let mut pwm_green = Pwm::new_output_a(peripherals.PWM_SLICE2, peripherals.PIN_4, config_green.clone());
    // Initialize PWM for blue LED
    let mut pwm_blue = Pwm::new_output_a(peripherals.PWM_SLICE3, peripherals.PIN_6, config_blue.clone());

    // Potentiometer initialization
    let adc = Adc::new(peripherals.ADC, Irqs, AdcConfig::default());
    let potentiometer = AdcChannel::new_pin(peripherals.PIN_26, Pull::None);

    // Display initialization
    let sda = peripherals.PIN_0;
    let scl = peripherals.PIN_1;

    let mut config = I2cConfig::default();
    config.frequency = DISPLAY_FREQ;
    let i2c: I2c<'_, _, Blocking> = I2c::new_blocking(peripherals.I2C0, scl, sda, config);

    let interface = I2CDisplayInterface::new(i2c);
    let mut display: Ssd1306<_, _, BufferedGraphicsMode<_>> = Ssd1306::new(interface, DisplaySize128x64, DisplayRotation::Rotate0).into_buffered_graphics_mode();
    display.init().unwrap();
    display.flush().unwrap();
    display.clear();

    //Display the name of the project cause the sd card is not working
    let mut filename = "meloled";
    let style = MonoTextStyle::new(&FONT_10X20, BinaryColor::On);
    Text::new(filename, Point::new(0, 32), style).draw(&mut display).unwrap();
    display.flush().unwrap();


    // Initialize button for controlling LED color
    let mut button = Input::new(peripherals.PIN_16, Pull::Up);
    // Variable for keeping track of current color
    let mut color: LedColor = LedColor::Red;

    // // Read MP3 filenames from the SD card
    // let mut filenames = Vec::<String<32>, 16>::new();
    // for entry in controller.iterate_dir(&volume, &root_dir).unwrap() {
    //     if let DirEntry::File(ref name, _) = entry {
    //         if name.ends_with(".mp3") {
    //             filenames.push(name.clone()).unwrap();
    //         }
    //     }
    // }

    loop {
        // Display each filename on the screen for 5 seconds
        // for filename in filenames.iter() {
        //     display.clear();
        //     let style = MonoTextStyle::new(&FONT_10X20, BinaryColor::On);
        //     Text::new(filename, Point::new(0, 32), style).draw(&mut display).unwrap();
        //     display.flush().unwrap();
        //     Timer::after(Duration::from_secs(5)).await;
        // }
        
        //Here I should add the buttons commands but if the sdcard is not working, i can't use them...

        // Button check & color modifying
        button.wait_for_falling_edge().await;
        match color {
            LedColor::Red => {
                config_red.compare_b = TOP;
                config_green.compare_a = 0;
                config_blue.compare_a = 0;
                color = LedColor::Yellow;
            },
            LedColor::Yellow => {
                config_red.compare_b = TOP;
                config_green.compare_a = TOP;
                config_blue.compare_a = 0;
                color = LedColor::Blue;
            },
            LedColor::Blue => {
                config_red.compare_b = 0;
                config_green.compare_a = 0;
                config_blue.compare_a = TOP;
                color = LedColor::Red;
            },
        }
        pwm_red.set_config(&config_red);
        pwm_green.set_config(&config_green);
        pwm_blue.set_config(&config_blue);
        // Delay before the next iteration
        Timer::after(Duration::from_millis(100)).await;
    }
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}
