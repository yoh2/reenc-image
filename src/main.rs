use clap::Parser;
use image::{DynamicImage, ImageError, ImageReader};
use std::{
    fs::File,
    io::{self, BufReader, Read, Write},
    path::{Path, PathBuf},
};

/// Encode images to shrink them, if possible, by re-saving them with different encoders and settings.
#[derive(Debug, Parser)]
struct App {
    /// The target size in bytes
    #[clap(short = 's', long, default_value = "15728640")]
    target_size: usize,

    /// Overwrite existing files with the same name as the encoded ones (if they exist)
    #[clap(short = 'f', long)]
    force: bool,

    /// Wait for user input before exiting (useful when launched from a file manager)
    #[clap(short = 'w', long, default_value_t = cfg!(windows), action = clap::ArgAction::Set)]
    wait: bool,

    /// The input image files to be re-encoded
    #[clap(required = true)]
    images: Vec<PathBuf>,
}

fn main() {
    let app = App::parse();
    println!("Target size: {} bytes", app.target_size);

    for image in &app.images {
        print!("Re-encoding {} ", image.display());
        io::stdout().flush().unwrap();

        match re_encode_image(image, app.target_size, app.force) {
            Ok(EncodeOutcome::Encoded {
                original_size,
                new_size,
                new_path,
            }) => {
                println!(
                    " ({original_size} bytes) -> {} ({new_size} bytes)",
                    new_path.display()
                );
            }
            Ok(EncodeOutcome::Skipped { original_size }) => {
                println!(" ({original_size} bytes) -> (skipped, already smaller than target)");
            }
            Err(e) => {
                println!("-> *** error: {e}");
            }
        }
    }

    if app.wait {
        // Wait for user input before exiting, so they can see the results
        let _ = io::stdin().read_exact(&mut [0]);
    }
}

#[derive(Debug, thiserror::Error)]
enum Error {
    #[error("Failed to process image: {0}")]
    Image(#[from] ImageError),

    #[error("io error: {0}")]
    Io(#[from] io::Error),

    #[error("Image size exceeds target size after encoded")]
    ImageSizeExceedsTarget,
}

#[derive(Debug)]
enum EncodeOutcome {
    Encoded {
        original_size: u64,
        new_size: u64,
        new_path: PathBuf,
    },
    Skipped {
        original_size: u64,
    },
}

fn re_encode_image(
    image_path: &Path,
    target_size: usize,
    force_overwrite: bool,
) -> Result<EncodeOutcome, Error> {
    let file = File::open(image_path)?;
    let original_size = file.metadata()?.len();

    if original_size < target_size as u64 {
        return Ok(EncodeOutcome::Skipped { original_size });
    }

    let image = ImageReader::new(BufReader::new(file))
        .with_guessed_format()?
        .decode()?;

    for strategy in ENCODE_STRATEGIES {
        let (encoded_data, extension) = strategy(&image)?;
        if encoded_data.len() >= target_size {
            continue;
        }

        let mut new_file_name = image_path
            .file_stem()
            .expect("image_path must be a file")
            .to_os_string();
        new_file_name.push("-reenc");
        new_file_name.push(extension);
        let new_path = image_path.with_file_name(new_file_name);

        let mut file = if force_overwrite {
            File::create(&new_path)
        } else {
            File::create_new(&new_path)
        }?;
        file.write_all(&encoded_data)?;

        return Ok(EncodeOutcome::Encoded {
            original_size,
            new_size: encoded_data.len() as u64,
            new_path,
        });
    }

    Err(Error::ImageSizeExceedsTarget)
}

macro_rules! def_encode_fn {
    ($fn_name:ident, $create_encoder:expr, $extension:expr) => {
        fn $fn_name(image: &DynamicImage) -> Result<(Vec<u8>, &'static str), Error> {
            let mut buf = Vec::new();
            let encoder = $create_encoder(&mut buf);
            image.write_with_encoder(encoder)?;
            Ok((buf, $extension))
        }
    };
}

def_encode_fn!(
    encode_to_png,
    |w| {
        use image::codecs::png::{CompressionType, FilterType, PngEncoder};
        PngEncoder::new_with_quality(w, CompressionType::Best, FilterType::Adaptive)
    },
    ".png"
);

def_encode_fn!(
    encode_to_jpeg_100,
    |w| {
        use image::codecs::jpeg::JpegEncoder;
        JpegEncoder::new_with_quality(w, 100)
    },
    ".jpg"
);

def_encode_fn!(
    encode_to_jpeg_90,
    |w| {
        use image::codecs::jpeg::JpegEncoder;
        JpegEncoder::new_with_quality(w, 90)
    },
    ".jpg"
);

type EncodeFn = fn(&DynamicImage) -> Result<(Vec<u8>, &'static str), Error>;

const ENCODE_STRATEGIES: &[EncodeFn] = &[encode_to_png, encode_to_jpeg_100, encode_to_jpeg_90];

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::sync::atomic::{AtomicUsize, Ordering};

    // Helper that automatically removes its temporary directory on drop
    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> Self {
            static COUNTER: AtomicUsize = AtomicUsize::new(0);
            let id = COUNTER.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "reenc-image-test-{}-{}",
                std::process::id(),
                id
            ));
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// Saves a 300x300 solid RGB image as BMP (~270KB).
    /// Large enough to exceed target_size=200_000 and trigger encode.
    fn create_large_bmp(dir: &Path) -> PathBuf {
        let img = image::DynamicImage::new_rgb8(300, 300);
        let path = dir.join("test.bmp");
        img.save(&path).unwrap();
        path
    }

    /// Saves a 10x10 solid RGB image as PNG (a few hundred bytes).
    fn create_small_png(dir: &Path) -> PathBuf {
        let img = image::DynamicImage::new_rgb8(10, 10);
        let path = dir.join("test.png");
        img.save(&path).unwrap();
        path
    }

    // Files smaller than target_size are skipped, returning the original size.
    #[test]
    fn test_skip_when_already_small() {
        let dir = TempDir::new();
        let path = create_small_png(dir.path());
        let orig_size = path.metadata().unwrap().len();

        let result = re_encode_image(&path, usize::MAX, false).unwrap();

        assert!(
            matches!(result, EncodeOutcome::Skipped { original_size } if original_size == orig_size)
        );
    }

    // On successful encoding, the output filename gets a -reenc suffix
    // and its size is below target_size.
    #[test]
    fn test_encoded_filename_and_size() {
        let dir = TempDir::new();
        let path = create_large_bmp(dir.path());
        let target = 200_000usize;

        let result = re_encode_image(&path, target, false).unwrap();

        match result {
            EncodeOutcome::Encoded {
                new_size, new_path, ..
            } => {
                let stem = new_path.file_stem().unwrap().to_str().unwrap();
                assert!(
                    stem.ends_with("-reenc"),
                    "stem should end with -reenc, got: {stem}"
                );
                assert!(new_path.exists(), "output file should exist on disk");
                assert!(
                    (new_size as usize) < target,
                    "new_size {new_size} should be < target {target}"
                );
            }
            EncodeOutcome::Skipped { .. } => panic!("expected Encoded, got Skipped"),
        }
    }

    // Returns ImageSizeExceedsTarget error if no strategy can meet the target.
    #[test]
    fn test_error_all_strategies_fail() {
        let dir = TempDir::new();
        let path = create_large_bmp(dir.path());

        let result = re_encode_image(&path, 1, false);

        assert!(matches!(result, Err(Error::ImageSizeExceedsTarget)));
    }

    // Returns an IO error if the output already exists and force=false.
    #[test]
    fn test_error_output_already_exists() {
        let dir = TempDir::new();
        let path = create_large_bmp(dir.path());
        // PNG is tried first, so pre-create test-reenc.png to trigger the error.
        File::create(dir.path().join("test-reenc.png")).unwrap();

        let result = re_encode_image(&path, 200_000, false);

        assert!(matches!(result, Err(Error::Io(_))));
    }

    // Overwrites the existing output and succeeds if force=true.
    #[test]
    fn test_force_overwrites_existing_output() {
        let dir = TempDir::new();
        let path = create_large_bmp(dir.path());
        File::create(dir.path().join("test-reenc.png")).unwrap();

        let result = re_encode_image(&path, 200_000, true).unwrap();

        assert!(matches!(result, EncodeOutcome::Encoded { .. }));
    }
}
