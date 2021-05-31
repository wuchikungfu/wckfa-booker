use std::fmt;

use std::io;
use std::io::BufWriter;
use std::io::Write;

use std::fs::File;

use std::path::Path;

extern crate clap;
use clap::App;
use clap::Arg;

extern crate walkdir;
use walkdir::WalkDir;

extern crate exif;

extern crate chrono;
use chrono::NaiveDateTime;
use chrono::NaiveDate;

extern crate tempdir;
use tempdir::TempDir;

extern crate image;
use image::imageops;
use image::imageops::FilterType;

extern crate printpdf;
use printpdf::*;

const VERSION: &'static str = env!("CARGO_PKG_VERSION");
const AUTHORS: &'static str = env!("CARGO_PKG_AUTHORS");
const DESCRIPTION: &'static str = env!("CARGO_PKG_DESCRIPTION");

#[derive(Debug)]
struct ImageAndMetadata {
  path: String,
  date_created: NaiveDateTime
}

impl fmt::Display for ImageAndMetadata {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    write!(f, "{} {}", self.path, self.date_created)
  }
}


fn main() {
  // We need the following command line arguments:
  // -i <input directory>
  // -o <output file (pdf)>
  let matches = App::new("wckfa-booker")
                .author(AUTHORS)
                .version(VERSION)
                .about(DESCRIPTION)
                .arg(Arg::with_name("input")
                  .short("i")
                  .long("input")
                  .value_name("input_directory")
                  .help("Specifies the input directory from which source images should be taken")
                  .takes_value(true)
                  .required(true))
                .arg(Arg::with_name("output")
                  .short("o")
                  .long("output")
                  .value_name("outfile")
                  .help("Specifies the name of the file to create as output")
                  .takes_value(true)
                  .required(true))
                .arg(Arg::with_name("title")
                  .short("t")
                  .long("title")
                  .value_name("title")
                  .help("Specifies the title of the final PDF")
                  .takes_value(true)
                  .required(true))
                .get_matches();

  // Eventually, we'd like to also accept:
  // -a <author name>

  // Calling .unwrap() is safe here because "INPUT" is required (if "INPUT" wasn't
  // required we could have used an 'if let' to conditionally get the value)
  let input_dir = matches.value_of("input").unwrap();
  let output_file = matches.value_of("output").unwrap();
  let doc_title = matches.value_of("title").unwrap();

  let vals_option = process_input_files(input_dir);
  let mut vals = vals_option.unwrap();

  // Sort vals by datetime
  vals.sort_by(|a, b| a.date_created.partial_cmp(&b.date_created).unwrap());

  // Create a temporary directory to write to
  let tmp_dir = TempDir::new("wckfa-booker").unwrap();

  let total_images = &vals.len();

  let mut next_page = 1;
  for next_value in vals {
    print!("Processing page {} of {}...", next_page, total_images);
    io::stdout().flush().ok().expect("Could not flush stdout");

    let page_name = format!("page-{:03}.jpg", next_page);

    // output a grayscale version of the image to the temporary directory
    let img = image::open(&next_value.path).unwrap();
    let rgb16 = img.to_rgb8();
    let (width, height) = rgb16.dimensions();
    let working_image;
    if width > height {
      // Image is in landscape mode. We need to rotate it.
      working_image = imageops::rotate270(&rgb16);
    } else {
      working_image = rgb16;
    }

    let grayscale_image = imageops::grayscale(&working_image);

    // resize the image to an appropriate size for 8.5x11" paper
    let resized_image = imageops::resize(&grayscale_image, 1275, 1650, FilterType::CatmullRom);
    resized_image.save(tmp_dir.path().join(page_name)).unwrap();
    next_page = next_page + 1;
    print!("Done\n");
    io::stdout().flush().ok().expect("Could not flush stdout");
  }

  write_images_to_pdf_file(tmp_dir.path().to_str().unwrap(), Path::new(output_file), doc_title, total_images);

  // By closing the `TempDir` explicitly, we can check that it has
  // been deleted successfully. If we don't close it explicitly,
  // the directory will still be deleted when `tmp_dir` goes out
  // of scope, but we won't know whether deleting the directory
  // succeeded.
  tmp_dir.close().unwrap();
}

fn write_images_to_pdf_file(input_dir_name: &str, output_file: &Path, doc_title: &str, num_images: &usize) {
  let (mut doc, first_page_idx, first_layer_idx) = PdfDocument::new(doc_title, Mm(216.0), Mm(279.0), "Layer 1");
  doc = doc.with_conformance(PdfConformance::Custom(CustomPdfConformance {
    requires_icc_profile: false,
    requires_xmp_metadata: false,
      .. Default::default()
    }));

  let mut current_page = doc.get_page(first_page_idx);
  let mut current_layer = current_page.get_layer(first_layer_idx);

  let mut current_image = 1;
  while current_image <= *num_images {
    let page_image = format!("page-{:03}.jpg", current_image);
    let image_file = Path::new(input_dir_name).join(page_image);
    print!("Writing image {} to PDF file...", image_file.to_str().unwrap());
    io::stdout().flush().ok().expect("Could not flush stdout");

    let mut image_file = File::open(image_file).unwrap();
    let image = Image::try_from(image::jpeg::JpegDecoder::new(&mut image_file).unwrap()).unwrap();
    image.add_to_layer(current_layer.clone(), None, None, None, Some(2.0), Some(2.0), None);

    if current_image + 1 <= *num_images {
      let (page_idx, layer_idx) = doc.add_page(Mm(216.0), Mm(279.0), format!("Page {}, Layer 1", current_image));
      current_page = doc.get_page(page_idx);
      current_layer = current_page.get_layer(layer_idx);
    }

    current_image = current_image + 1;
    print!("Done\n");
  }

  doc.save(&mut BufWriter::new(File::create(output_file).unwrap())).unwrap();
}

fn process_input_files(input: &str) -> Result<Vec<ImageAndMetadata>, exif::Error> {
  // Process each entry in the input directory and determine its size and when it was created.
  let partitioned_files = WalkDir::new(input)
    .into_iter()
    .filter_map(|e| {
      e.ok()
    })
    .filter(|e| {
      !e.file_type().is_dir()
    });

  let mut v: Vec<ImageAndMetadata> = Vec::new();

  for entry in partitioned_files {
    let imamd = retrieve_image_and_metadata(&entry.path().display().to_string());

    v.push(imamd?);
  }

  return Ok(v);
}

fn retrieve_image_and_metadata(image_file_path: &str) -> Result<ImageAndMetadata, exif::Error> {
  let file = std::fs::File::open(image_file_path)?;
  let mut bufreader = std::io::BufReader::new(&file);
  let exifreader = exif::Reader::new();
  let exif = exifreader.read_from_container(&mut bufreader)?;
  let f = exif.get_field(exif::Tag::DateTimeOriginal, exif::In::PRIMARY).unwrap();
  let date_time_str = f.display_value().with_unit(&exif).to_string();
  let split_date_time : Vec<&str> = date_time_str.split(' ').collect();
  let split_date : Vec<&str> = split_date_time[0].split('-').collect();
  let split_time : Vec<&str> = split_date_time[1].split(':').collect();
  let year = split_date[0].to_string().parse::<i32>().unwrap();
  let month = split_date[1].to_string().parse::<u32>().unwrap();
  let day = split_date[2].to_string().parse::<u32>().unwrap();
  let hours = split_time[0].to_string().parse::<u32>().unwrap();
  let minutes = split_time[1].to_string().parse::<u32>().unwrap();
  let seconds = split_time[2].to_string().parse::<u32>().unwrap();

  let date_time : NaiveDateTime = NaiveDate::from_ymd(year, month, day).and_hms(hours, minutes, seconds);

  return Ok(
    ImageAndMetadata {
      path: image_file_path.to_string(),
      date_created: date_time
    }
  );
}
