mod tiff;

// use std::io::{self, Read, Write};
use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use tiff::{usizeify, Endian, EntryTag, EntryType, IFDEntry};

use tokio::runtime::Builder;
use tokio::time::{sleep, Duration};

use crossterm::{
    cursor, event, execute, queue, style,
    terminal::{self, ClearType},
};

use image::GenericImageView;

// use rand::Rng;

fn main() -> crossterm::Result<()> {
    let runtime = Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .unwrap();

    let mut handles = Vec::with_capacity(10);
    for i in 0..10 {
        handles.push(runtime.spawn(my_bg_task(i)));
    }

    // let _: Option<i32> = std::io::stdin()
    //     .bytes()
    //     .next()
    //     .and_then(|result| result.ok())
    //     .map(|byte| byte as i32);

    // let mut rng = rand::thread_rng();
    // let index = rng.gen_range(0..handles.len());
    // handles[index].abort();

    std::thread::sleep(Duration::from_millis(750));
    println!("Finished time-consuming task.");

    for handle in handles {
        // The `spawn` method returns a `JoinHandle`. A `JoinHandle` is a future so we can wait for
        // it using `block_on`.
        match runtime.block_on(handle) {
            Ok(_) => (),
            Err(_) => println!("A green thread was cancelled!"),
        }
    }

    let mut w = io::stdout();

    terminal::enable_raw_mode()?;

    queue!(w, terminal::EnterAlternateScreen)?;

    queue!(
        w,
        style::ResetColor,
        terminal::Clear(ClearType::All),
        cursor::Hide,
    )?;

    let (width, height) = terminal::size()?;
    let win_pixels = get_win_pixels()?;

    let left_x = width / 2;

    let file_path = PathBuf::from(".0_images/skrollok.jpg");
    // let file_path = PathBuf::from(".0_images/bash_icon_small.png");

    let can_display_image = Arc::new(Mutex::new(true));

    let image_handle = runtime.spawn(preview_image(
        win_pixels,
        file_path.clone(),
        file_path
            .extension()
            .unwrap()
            .to_str()
            .unwrap()
            .to_string()
            .to_lowercase(),
        width,
        height,
        left_x,
        Arc::clone(&can_display_image),
    ));

    let input = event::read()?;
    match input {
        event::Event::Key(key_event) => match key_event.code {
            event::KeyCode::Char('q') => {
                image_handle.abort();

                {
                    let mut can_display_image = can_display_image.lock().unwrap();

                    *can_display_image = false;
                }

                // FIXME(Chris): Remove blocking on the thread. In theory, we should never need to
                // wait for these async threads.
                // match runtime.block_on(image_handle) {
                //     Ok(_) => {
                //         execute!(
                //             w,
                //             cursor::MoveTo(left_x, 2),
                //             style::Print("The process finished before it was killed."),
                //         )?;
                //     },
                //     Err(_) => {
                //         execute!(
                //             w,
                //             cursor::MoveTo(left_x, 2),
                //             style::Print("The process was killed."),
                //         )?;
                //     },
                // }

                // w.write(b"\x1b_Ga=d;\x1b\\")?;
                // w.flush()?;

                let mut stdout_lock = w.lock();

                execute!(
                    stdout_lock,
                    cursor::MoveTo(left_x, 1),
                    style::Print("Aborted.             "),
                )?;
            }
            _ => {
                let _ = runtime.block_on(image_handle);
            }
        },
        _ => {
            let _ = runtime.block_on(image_handle);
        }
    }

    let _ = event::read()?;

    execute!(
        w,
        style::ResetColor,
        cursor::Show,
        terminal::LeaveAlternateScreen,
        cursor::MoveToNextLine(1),
    )?;

    terminal::disable_raw_mode()?;

    Ok(())
}

async fn my_bg_task(i: u64) {
    // By subtracting, the tasks with larger values of i sleep for a shorter duration
    let millis = 1000 - 50 * i;
    println!("Task {} sleeping for {} ms.", i, millis);

    sleep(Duration::from_millis(millis)).await;

    println!("Task {} stopping.", i);
}

#[derive(Debug, Clone, Copy)]
struct WindowPixels {
    width: u16,
    height: u16,
}

// NOTE(Chris): As long as the only rendering side effect of this function is to display an image,
// we should be fine just clearing all displayed images after blocking on this task.
async fn preview_image(
    win_pixels: WindowPixels,
    third_file: PathBuf,
    ext: String,
    width: u16,
    height: u16,
    left_x: u16,
    can_display_image: Arc<Mutex<bool>>,
) -> std::io::Result<()> {
    // TODO(Chris): Load and display images asynchronously to allow more
    // input while scrolling through images
    // TODO(Chris): Improve the image quality of previews
    // TODO(Chris): Eliminate resizing artifacts when images fit within
    // the third column

    {
        let stdout = io::stdout();
        let mut w = stdout.lock();

        // viuer::print(&img, &conf).expect("Image printing failed.");
        // viuer::print_to(&img, &conf, &mut w).expect("Image printing failed");
        queue!(w, cursor::MoveTo(left_x, 1), style::Print("Loading..."))?;

        w.flush()?;
    }

    let win_px_width = win_pixels.width;
    let win_px_height = win_pixels.height;

    // TODO(Chris): Look into using libjpeg-turbo (https://github.com/ImageOptim/mozjpeg-rust)
    // to decode large jpegs faster
    let mut img = image::io::Reader::open(&third_file)?.decode().unwrap();

    // NOTE(Chris): sxiv only rotates jpgs somewhat-correctly, but Eye of
    // Gnome (eog) rotates them correctly

    // Rotate jpgs according to their orientation value
    // One-iteration loop for early break
    loop {
        if ext == "jpg" || ext == "jpeg" {
            let bytes = std::fs::read(&third_file)?;

            // Find the location of the Exif header
            let exif_header = b"Exif\x00\x00";
            let exif_header_index = match tiff::find_bytes(&bytes, exif_header) {
                Some(value) => value,
                None => break,
            };

            // This assumes that the beginning of the TIFF section
            // comes right after the Exif header
            let tiff_index = exif_header_index + exif_header.len();
            let tiff_bytes = &bytes[tiff_index..];

            let byte_order = match &tiff_bytes[0..=1] {
                b"II" => Endian::LittleEndian,
                b"MM" => Endian::BigEndian,
                _ => panic!("Unable to determine endianness of TIFF section!"),
            };

            if tiff_bytes[2] != 42 && tiff_bytes[3] != 42 {
                panic!("Could not confirm existence of TIFF section with 42!");
            }

            // From the beginning of the TIFF section
            let first_ifd_offset = usizeify(&tiff_bytes[4..=7], byte_order);

            let num_ifd_entries = usizeify(
                &tiff_bytes[first_ifd_offset..first_ifd_offset + 2],
                byte_order,
            );

            let first_ifd_entry_offset = first_ifd_offset + 2;

            // NOTE(Chris): We don't actually need info on all of the
            // IFD entries, but I'm too lazy to break early from the
            // for loop
            let mut ifd_entries = vec![];
            for entry_index in 0..num_ifd_entries {
                let entry_bytes = &tiff_bytes[first_ifd_entry_offset + (12 * entry_index)..];
                let entry = IFDEntry::from_slice(entry_bytes, byte_order);
                ifd_entries.push(entry);
            }

            let orientation_ifd = ifd_entries.iter().find(|entry| {
                entry.tag == EntryTag::Orientation
                    && entry.field_type == EntryType::Short
                    && entry.count == 1
            });

            let orientation_value = match orientation_ifd {
                Some(value) => value,
                None => break,
            };

            match orientation_value.value_offset {
                1 => (),
                2 => img = img.fliph(),
                3 => img = img.rotate180(),
                4 => img = img.flipv(),
                5 => img = img.rotate90().fliph(),
                6 => img = img.rotate90(),
                7 => img = img.rotate270().fliph(),
                8 => img = img.rotate270(),
                _ => (),
            }

            tiff::IFDEntry::from_slice(&bytes, byte_order);
        }

        break;
    }

    let (img_width, img_height) = img.dimensions();

    let mut img_cells_width = img_width * (width as u32) / (win_px_width as u32);
    let mut img_cells_height = img_height * (height as u32) / (win_px_height as u32);

    let orig_img_cells_width = img_cells_width;

    // let third_column_width = width - left_x - 2;

    // Subtract 1 because columns start at y = 1, subtract 1 again
    // because columns stop at the penultimate row
    let third_column_height = (height - 2) as u32;

    // Scale the image down to fit the width, if necessary
    if (left_x as u32) + img_cells_width >= (width as u32) {
        img_cells_width = (width - left_x - 2) as u32;
    }

    // Scale the image even further down to fit the height, if
    // necessary
    let new_cells_height = img_cells_height / (orig_img_cells_width / img_cells_width);
    if new_cells_height > third_column_height {
        let display_cells_height = new_cells_height / 2;
        img_cells_width = orig_img_cells_width / (img_cells_height / display_cells_height);
        img_cells_height = display_cells_height;
    }

    if orig_img_cells_width != img_cells_width {
        let display_width_px = img_cells_width * (win_px_width as u32) / (width as u32);
        let display_height_px = img_cells_height * (win_px_height as u32) / (height as u32);

        img = img.thumbnail(display_width_px, display_height_px);
    }

    let stdout = io::stdout();
    let mut w = stdout.lock();

    let rgba = img.to_rgba8();
    let raw_img = rgba.as_raw();
    let path = store_in_tmp_file(raw_img)?;

    // This scope exists to eventually unlock the mutex
    {
        let can_display_image = can_display_image.lock().unwrap();

        if *can_display_image {
            execute!(
                w,
                cursor::MoveTo(left_x, 1),
                style::Print("Should display!")
            )?;

            queue!(w, cursor::MoveTo(left_x, 1))?;

            write!(
                w,
                "\x1b_Gf=32,s={},v={},a=T,t=t;{}\x1b\\",
                img.width(),
                img.height(),
                base64::encode(path.to_str().unwrap())
            )?;
        }
    }

    w.flush()?;

    // queue!(
    //     w,
    //     cursor::MoveTo(left_x, 21),
    //     style::Print("preview_image has finished.")
    // )?;

    w.flush()?;

    Ok(())
}

// Create a file in temporary dir and write the byte slice to it.
fn store_in_tmp_file(buf: &[u8]) -> std::result::Result<std::path::PathBuf, io::Error> {
    let (mut tmpfile, path) = tempfile::Builder::new()
        .prefix(".tmp.rolf")
        .rand_bytes(1)
        .tempfile()?
        // Since the file is persisted, the user is responsible for deleting it afterwards. However,
        // Kitty does this automatically after printing from a temp file.
        .keep()?;

    tmpfile.write_all(buf)?;
    tmpfile.flush()?;
    Ok(path)
}

// A Linux-specific, possibly-safe wrapper around an ioctl call with TIOCGWINSZ.
// Gets the width and height of the terminal in pixels.
fn get_win_pixels() -> std::result::Result<WindowPixels, io::Error> {
    let win_pixels = unsafe {
        let mut winsize = libc::winsize {
            ws_col: 0,
            ws_row: 0,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };

        // NOTE(Chris): From Linux's man ioctl_tty
        const TIOCGWINSZ: u64 = 21523;

        // 0 is the file descriptor for stdin
        let err = libc::ioctl(0, TIOCGWINSZ, &mut winsize);
        if err != 0 {
            let errno_location = libc::__errno_location();
            let errno = (*errno_location) as i32;

            return Err(io::Error::from_raw_os_error(errno));

            // panic!("Failed to get the size of terminal window in pixels.");
        }

        WindowPixels {
            width: winsize.ws_xpixel,
            height: winsize.ws_ypixel,
        }
    };

    Ok(win_pixels)
}
