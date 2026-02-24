use clap::Parser;
use image::{DynamicImage, ImageError, ImageReader};
use std::{
    fs::File,
    io::{self, BufReader, Read, Write},
    path::{Path, PathBuf},
};

/// Convert images to shrink them, if possible, by re-saving them with different encoders and settings.
#[derive(Debug, Parser)]
struct App {
    /// The target size in bytes
    #[clap(short = 's', long, default_value = "15728640")]
    target_size: usize,

    /// The input image files to be re-converted
    #[clap(required = true)]
    images: Vec<PathBuf>,
}

fn main() {
    let app = App::parse();
    println!("Target size: {} bytes", app.target_size);

    for image in &app.images {
        print!("Re-converting {} ", image.display());
        match re_convert_image(image, app.target_size) {
            Ok(ConversionOutcome::Converted {
                original_size,
                new_size,
                new_path,
            }) => {
                println!(
                    " ({original_size} bytes) -> {} ({new_size} bytes)",
                    new_path.display()
                );
            }
            Ok(ConversionOutcome::Skipped { original_size }) => {
                println!(" ({original_size} bytes) -> (skipped, already smaller than target)");
            }
            Err(e) => {
                println!("-> *** error: {e}");
            }
        }
    }

    // Wait for user input before exiting, so they can see the results
    io::stdin().read_exact(&mut [0]).unwrap();
}

#[derive(Debug, thiserror::Error)]
enum Error {
    #[error("Failed to process image: {0}")]
    Image(#[from] ImageError),

    #[error("io error: {0}")]
    Io(#[from] io::Error),

    #[error("Image size exceeds target size after conversion")]
    ImageSizeExceedsTarget,
}

#[derive(Debug)]
enum ConversionOutcome {
    Converted {
        original_size: u64,
        new_size: u64,
        new_path: PathBuf,
    },
    Skipped {
        original_size: u64,
    },
}

fn re_convert_image(image_path: &Path, target_size: usize) -> Result<ConversionOutcome, Error> {
    let file = File::open(image_path)?;
    let original_size = file.metadata()?.len();

    if original_size < target_size as u64 {
        return Ok(ConversionOutcome::Skipped { original_size });
    }

    let image = ImageReader::new(BufReader::new(file))
        .with_guessed_format()?
        .decode()?;

    for strategy in CONVERSION_STRATEGIES {
        let (converted_data, extension) = strategy(&image)?;
        if converted_data.len() >= target_size {
            continue;
        }

        let mut new_file_name = image_path
            .file_stem()
            .expect("image_path must be a file")
            .to_os_string();
        new_file_name.push("-reconv");
        new_file_name.push(extension);
        let new_path = image_path.with_file_name(new_file_name);

        File::create_new(&new_path)?.write_all(&converted_data)?;

        return Ok(ConversionOutcome::Converted {
            original_size,
            new_size: converted_data.len() as u64,
            new_path,
        });
    }

    Err(Error::ImageSizeExceedsTarget)
}

macro_rules! def_conversion_fn {
    ($fn_name:ident, $create_encoder:expr, $extension:expr) => {
        fn $fn_name(image: &DynamicImage) -> Result<(Vec<u8>, &'static str), Error> {
            let mut buf = Vec::new();
            let encoder = $create_encoder(&mut buf);
            image.write_with_encoder(encoder)?;
            Ok((buf, $extension))
        }
    };
}

def_conversion_fn!(
    convert_to_png,
    |w| {
        use image::codecs::png::{CompressionType, FilterType, PngEncoder};
        PngEncoder::new_with_quality(w, CompressionType::Best, FilterType::Adaptive)
    },
    ".png"
);

def_conversion_fn!(
    convert_to_jpeg_100,
    |w| {
        use image::codecs::jpeg::JpegEncoder;
        JpegEncoder::new_with_quality(w, 100)
    },
    ".jpg"
);

def_conversion_fn!(
    convert_to_jpeg_90,
    |w| {
        use image::codecs::jpeg::JpegEncoder;
        JpegEncoder::new_with_quality(w, 90)
    },
    ".jpg"
);

type ConversionFn = fn(&DynamicImage) -> Result<(Vec<u8>, &'static str), Error>;

const CONVERSION_STRATEGIES: &[ConversionFn] =
    &[convert_to_png, convert_to_jpeg_100, convert_to_jpeg_90];
