use crate::config::OcrPaths;
use anyhow::{Context, Result, ensure};
use ort::{
    session::Session,
    tensor::TensorElementType,
    value::{Tensor, ValueType},
};

const CLASSES: usize = 8210;
const HEIGHT: usize = 64;
const MAX_WIDTH: usize = 4096;

pub struct Ocr {
    session: Session,
    dictionary: Vec<String>,
}

impl Ocr {
    pub fn load(paths: &OcrPaths) -> Result<Self> {
        for (name, path) in [
            ("model", &paths.model),
            ("dictionary", &paths.dictionary),
            ("runtime", &paths.runtime),
        ] {
            ensure!(
                path.is_file(),
                "OCR {name} file does not exist: {}",
                path.display()
            );
        }
        #[derive(serde::Deserialize)]
        struct CharsetConfig {
            charset: Vec<String>,
            word: bool,
            image: [i64; 2],
            channel: usize,
        }
        let config: CharsetConfig = serde_json::from_slice(&std::fs::read(&paths.dictionary)?)
            .context("Expected ddddocr charset configuration JSON")?;
        ensure!(
            !config.word && config.image == [-1, 64] && config.channel == 1,
            "Expected ddddocr grayscale, dynamic-width, height-64 configuration"
        );
        let dictionary = config.charset;
        ensure!(
            dictionary.len() == CLASSES && dictionary[0].is_empty(),
            "Expected ddddocr 8210-class charset with empty blank at index zero"
        );
        let runtime = std::fs::canonicalize(&paths.runtime)?;
        ort::init_from(runtime.to_string_lossy())
            .commit()
            .context("Cannot initialize ONNX Runtime")?;
        let session = Session::builder()?
            .with_intra_threads(1)?
            .commit_from_file(&paths.model)
            .context("Cannot load OCR model")?;
        ensure!(
            session.inputs.len() == 1 && session.inputs[0].name == "input1",
            "Expected one OCR input named input1"
        );
        ensure!(
            matches!(&session.inputs[0].input_type, ValueType::Tensor { ty: TensorElementType::Float32, shape, .. }
            if shape.len() == 4 && (shape[0] == -1 || shape[0] == 1) && shape[1] == 1 && shape[2] == 64 && shape[3] == -1),
            "Expected dynamic float32 OCR input [N,1,64,W]"
        );
        // These exported models have inaccurate static output shape metadata.
        // Validate the actual tensor dimensions after inference instead.
        ensure!(session.outputs.len() == 1, "Expected one ddddocr output");
        Ok(Self {
            session,
            dictionary,
        })
    }

    /// Decode all model classes; no digit filtering or prediction repair.
    pub fn recognize_raw(&mut self, bytes: &[u8]) -> Result<String> {
        let input = preprocess(bytes)?;
        let outputs = self
            .session
            .run(ort::inputs!["input1" => Tensor::from_array(input)?])?;
        let (shape, scores) = outputs[0].try_extract_tensor::<f32>()?;
        decode_ctc(shape, scores, &self.dictionary)
    }

    pub fn recognize(&mut self, bytes: &[u8]) -> Result<String> {
        validate_prediction(self.recognize_raw(bytes)?)
    }
}

fn validate_prediction(prediction: String) -> Result<String> {
    ensure!(
        prediction.len() == 4 && prediction.bytes().all(|b| b.is_ascii_digit()),
        "OCR prediction must be exactly four ASCII digits; received {prediction:?}"
    );
    Ok(prediction)
}

fn decode_ctc(shape: &[i64], scores: &[f32], dictionary: &[String]) -> Result<String> {
    ensure!(
        shape.len() == 3 && shape[0] > 0 && shape[1] == 1 && shape[2] == dictionary.len() as i64,
        "Invalid OCR output dimensions: {shape:?}"
    );
    let classes = dictionary.len();
    ensure!(
        classes > 0 && (shape[0] as usize).checked_mul(classes) == Some(scores.len()),
        "Invalid OCR output storage length"
    );
    let mut previous = None;
    let mut result = String::new();
    for step in scores.chunks_exact(classes) {
        ensure!(step.iter().all(|v| v.is_finite()), "Non-finite OCR scores");
        let mut best = 0;
        for index in 1..classes {
            if step[index] > step[best] {
                best = index;
            }
        }
        if previous != Some(best) && best != 0 {
            result.push_str(&dictionary[best]);
        }
        previous = Some(best);
    }
    Ok(result)
}

fn preprocess(bytes: &[u8]) -> Result<ndarray::Array4<f32>> {
    ensure!(
        !bytes.is_empty() && bytes.len() <= 8 * 1024 * 1024,
        "OCR image must be between 1 byte and 8 MiB"
    );
    let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format()?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(4096);
    limits.max_image_height = Some(4096);
    limits.max_alloc = Some(64 * 1024 * 1024);
    reader.limits(limits);
    let rgb = reader
        .decode()
        .context("Cannot decode CAPTCHA image")?
        .to_rgb8();
    let (width, height) = rgb.dimensions();
    ensure!(width > 0 && height > 0, "Empty OCR image");
    let resized_width = HEIGHT * width as usize / height as usize;
    ensure!(
        resized_width > 0 && resized_width <= MAX_WIDTH,
        "OCR image aspect ratio exceeds supported width"
    );
    let resized = image::imageops::resize(
        &rgb,
        resized_width as u32,
        HEIGHT as u32,
        image::imageops::FilterType::Lanczos3,
    );
    let mut input = ndarray::Array4::zeros((1, 1, HEIGHT, resized_width));
    for (x, y, pixel) in resized.enumerate_pixels() {
        // Pillow RGB -> L fixed-point coefficients, after RGB resize.
        let gray =
            (19595 * pixel[0] as u32 + 38470 * pixel[1] as u32 + 7471 * pixel[2] as u32 + 32768)
                >> 16;
        input[[0, 0, y as usize, x as usize]] = gray as f32 / 255.0;
    }
    Ok(input)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn decode(indices: &[usize]) -> String {
        let dictionary = ["", "0", "6", "8", "字"].map(str::to_owned);
        let mut scores = vec![0.0; indices.len() * dictionary.len()];
        for (t, &index) in indices.iter().enumerate() {
            scores[t * dictionary.len() + index] = 1.0;
        }
        decode_ctc(
            &[indices.len() as i64, 1, dictionary.len() as i64],
            &scores,
            &dictionary,
        )
        .unwrap()
    }
    #[test]
    fn repeats_collapse_before_blank_removal() {
        assert_eq!(decode(&[1, 1, 2, 0, 2, 3, 3]), "0668");
    }
    #[test]
    fn leading_zero_is_preserved() {
        assert_eq!(
            validate_prediction(decode(&[1, 2, 0, 2, 3])).unwrap(),
            "0668"
        );
    }
    #[test]
    fn no_filtering_or_repair() {
        assert_eq!(decode(&[1, 4, 2, 3]), "0字68");
        for text in ["0字68", "068", "00688", "１２３４", " 668"] {
            assert!(validate_prediction(text.into()).is_err());
        }
    }
    #[test]
    fn bad_dimensions_are_errors() {
        let d = vec![String::new(), "0".into()];
        for shape in [vec![1, 2], vec![1, 2, 2], vec![0, 1, 2], vec![1, 1, 3]] {
            assert!(decode_ctc(&shape, &[0., 1.], &d).is_err());
        }
        assert!(decode_ctc(&[1, 2, 2], &[0., 1.], &d).is_err());
        assert!(decode_ctc(&[1, 1, 2], &[f32::NAN, 1.], &d).is_err());
    }
    #[test]
    fn rejects_invalid_images() {
        assert!(preprocess(&[]).is_err());
        assert!(preprocess(b"bad image").is_err());
        assert!(preprocess(&vec![0; 8 * 1024 * 1024 + 1]).is_err());
    }
    #[test]
    fn wide_images_use_dynamic_canvas_and_extreme_ratios_fail() {
        for (width, height, expected_width) in
            [(400, 64, Some(400)), (4096, 1, None), (4097, 64, None)]
        {
            let rgb = image::RgbImage::new(width, height);
            let mut bytes = std::io::Cursor::new(Vec::new());
            rgb.write_to(&mut bytes, image::ImageFormat::Png).unwrap();
            let result = preprocess(bytes.get_ref());
            match expected_width {
                Some(w) => assert_eq!(result.unwrap().shape(), &[1, 1, 64, w]),
                None => assert!(result.is_err()),
            }
        }
    }
    #[test]
    fn grayscale_normalization_and_dynamic_width() {
        let rgb = image::RgbImage::from_pixel(2, 2, image::Rgb([255, 0, 0]));
        let mut bytes = std::io::Cursor::new(Vec::new());
        rgb.write_to(&mut bytes, image::ImageFormat::Png).unwrap();
        let a = preprocess(bytes.get_ref()).unwrap();
        assert_eq!(a.shape(), &[1, 1, 64, 64]);
        assert_eq!(a[[0, 0, 0, 0]], 76.0 / 255.0);
        assert_eq!(a[[0, 0, 63, 63]], 76.0 / 255.0);
    }
}
