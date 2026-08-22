using System.Windows.Media.Imaging;
using OpenCvSharp;
using OpenCvSharp.Dnn;
using OpenCvSharp.WpfExtensions;

namespace PhotoSite.Services.Faces;

/// <summary>
/// One face found in an image, in normalized 0..1 coordinates. Smile and
/// EyesOpen are 0..1 probabilities, or null when the expression models are
/// not on disk.
/// </summary>
public sealed record DetectedFace(
    double X,
    double Y,
    double Width,
    double Height,
    double Confidence,
    float[] Embedding,
    double? Smile = null,
    double? EyesOpen = null);

/// <summary>
/// Local face detection and recognition: YuNet finds the faces and their
/// five landmarks, SFace turns each aligned face into a 128-dimensional
/// embedding whose cosine similarity says whether two faces belong to the
/// same person. Both models come from the OpenCV Zoo (Apache-2.0) and
/// everything runs on this machine.
/// Two optional expression models score the same aligned crop: FER+ (ONNX
/// Model Zoo, MIT) for "smiling" via its happiness class, and
/// open-closed-eye-0001 (OpenVINO Open Model Zoo, Apache-2.0) for
/// "eyes open" on a patch around each eye.
/// </summary>
public sealed class FaceEngine : IDisposable
{
    /// <summary>
    /// Faces this small on the detection frame carry too few pixels for a
    /// meaningful embedding.
    /// </summary>
    private const int MinimumFacePixels = 20;

    private const float ScoreThreshold = 0.8f;
    private const float NmsThreshold = 0.3f;
    private const int AlignedSize = 112;

    /// <summary>
    /// The wrapped detector is created for one fixed input size, so every
    /// frame is letterboxed onto this square canvas and the detections are
    /// mapped back afterwards.
    /// </summary>
    private const int DetectionCanvasSize = 1024;

    /// <summary>
    /// Where SFace expects the eyes, nose and mouth corners to sit on its
    /// 112×112 input - the standard ArcFace alignment target.
    /// </summary>
    private static readonly Point2f[] ReferenceLandmarks =
    [
        new(38.2946f, 51.6963f),
        new(73.5318f, 51.5014f),
        new(56.0252f, 71.7366f),
        new(41.5493f, 92.3655f),
        new(70.7299f, 92.2041f)
    ];

    private static readonly string ModelDirectory = Path.Combine(
        AppContext.BaseDirectory,
        "tools",
        "models");

    private static readonly string DetectorModelPath = Path.Combine(
        ModelDirectory,
        "face_detection_yunet_2023mar.onnx");

    private static readonly string RecognizerModelPath = Path.Combine(
        ModelDirectory,
        "face_recognition_sface_2021dec.onnx");

    private static readonly string EmotionModelPath = Path.Combine(
        ModelDirectory,
        "emotion-ferplus-8.onnx");

    private static readonly string EyeStateModelPath = Path.Combine(
        ModelDirectory,
        "open_closed_eye.onnx");

    /// <summary>FER+ consumes a 64×64 grayscale face.</summary>
    private const int EmotionInputSize = 64;

    /// <summary>Happiness is class 1 of FER+'s eight emotions.</summary>
    private const int HappinessClassIndex = 1;

    /// <summary>
    /// open-closed-eye-0001 consumes a 32×32 BGR patch; on the 112×112
    /// aligned face that side length wraps the eye with a healthy margin.
    /// </summary>
    private const int EyePatchSize = 32;

    private readonly object gate = new();
    private FaceDetectorYN? detector;
    private Net? recognizer;
    private Net? emotionNet;
    private Net? eyeStateNet;

    public static bool ModelsAvailable =>
        File.Exists(DetectorModelPath) && File.Exists(RecognizerModelPath);

    /// <summary>
    /// The smile and eyes-open scoring is optional: without these two files
    /// a scan still finds and recognizes faces, just without expressions.
    /// </summary>
    public static bool ExpressionModelsAvailable =>
        File.Exists(EmotionModelPath) && File.Exists(EyeStateModelPath);

    public static string ModelDirectoryPath => ModelDirectory;

    /// <summary>
    /// Finds every face in the bitmap and computes its embedding. Safe to
    /// call from any thread; calls are serialized because the underlying
    /// sessions are not thread-safe.
    /// </summary>
    public IReadOnlyList<DetectedFace> Detect(BitmapSource image)
    {
        lock (gate)
        {
            EnsureLoaded();
            var activeDetector = detector!;
            using var frame = ToBgrMat(image);
            var frameWidth = frame.Width;
            var frameHeight = frame.Height;
            var scale = Math.Min(
                1.0,
                Math.Min(
                    (double)DetectionCanvasSize / frameWidth,
                    (double)DetectionCanvasSize / frameHeight));
            var resizedWidth = Math.Max(1, (int)Math.Round(frameWidth * scale));
            var resizedHeight = Math.Max(1, (int)Math.Round(frameHeight * scale));

            using var canvas = new Mat(
                new Size(DetectionCanvasSize, DetectionCanvasSize),
                MatType.CV_8UC3,
                Scalar.Black);
            using (var resized = new Mat())
            using (var target = new Mat(
                       canvas,
                       new Rect(0, 0, resizedWidth, resizedHeight)))
            {
                Cv2.Resize(frame, resized, new Size(resizedWidth, resizedHeight));
                resized.CopyTo(target);
            }

            using var faces = new Mat();
            activeDetector.Detect(canvas, faces);

            var results = new List<DetectedFace>();
            var rows = faces.Rows;
            for (var index = 0; index < rows; index++)
            {
                var x = faces.At<float>(index, 0);
                var y = faces.At<float>(index, 1);
                var width = faces.At<float>(index, 2);
                var height = faces.At<float>(index, 3);
                var score = faces.At<float>(index, 14);
                if (width < MinimumFacePixels || height < MinimumFacePixels)
                {
                    continue;
                }

                // The canvas coordinates map back to the source frame, where
                // the alignment for the embedding samples the full detail.
                var landmarks = new Point2f[5];
                for (var point = 0; point < 5; point++)
                {
                    landmarks[point] = new Point2f(
                        (float)(faces.At<float>(index, 4 + point * 2) / scale),
                        (float)(faces.At<float>(index, 5 + point * 2) / scale));
                }

                using var aligned = Align(frame, landmarks);
                if (aligned is null)
                {
                    continue;
                }

                var embedding = ComputeEmbedding(aligned);
                var (smile, eyesOpen) = ComputeExpressions(aligned);
                results.Add(new DetectedFace(
                    Math.Clamp(x / scale / frameWidth, 0, 1),
                    Math.Clamp(y / scale / frameHeight, 0, 1),
                    Math.Clamp(width / scale / frameWidth, 0, 1),
                    Math.Clamp(height / scale / frameHeight, 0, 1),
                    score,
                    FaceMath.Normalize(embedding),
                    smile,
                    eyesOpen));
            }

            return results;
        }
    }

    /// <summary>
    /// Warps the face so its landmarks land on the reference positions - the
    /// same alignment OpenCV's FaceRecognizerSF performs. Every downstream
    /// network reads this one 112×112 crop, where the eyes and mouth sit at
    /// known coordinates.
    /// </summary>
    private static Mat? Align(Mat frame, Point2f[] landmarks)
    {
        using var transform = Cv2.EstimateAffinePartial2D(
            InputArray.Create(landmarks),
            InputArray.Create(ReferenceLandmarks));
        if (transform is null || transform.Empty())
        {
            return null;
        }

        var aligned = new Mat();
        Cv2.WarpAffine(
            frame,
            aligned,
            transform,
            new Size(AlignedSize, AlignedSize));
        return aligned;
    }

    private float[] ComputeEmbedding(Mat aligned)
    {
        // SFace consumes the raw BGR crop; no scaling or mean subtraction,
        // matching cv::FaceRecognizerSF::feature.
        using var blob = CvDnn.BlobFromImage(aligned);
        recognizer!.SetInput(blob);
        using var output = recognizer.Forward();
        var embedding = new float[output.Total()];
        for (var index = 0; index < embedding.Length; index++)
        {
            embedding[index] = output.At<float>(0, index);
        }

        return embedding;
    }

    /// <summary>
    /// Scores the aligned face for "smiling" and "eyes open". Nulls when the
    /// optional models are absent; a failure in either network only costs
    /// that one score, never the face.
    /// </summary>
    private (double? Smile, double? EyesOpen) ComputeExpressions(Mat aligned)
    {
        if (emotionNet is null || eyeStateNet is null)
        {
            return (null, null);
        }

        double? smile = null;
        double? eyesOpen = null;
        try
        {
            smile = ComputeSmile(aligned);
        }
        catch (Exception)
        {
        }

        try
        {
            eyesOpen = ComputeEyesOpen(aligned);
        }
        catch (Exception)
        {
        }

        return (smile, eyesOpen);
    }

    /// <summary>
    /// FER+ over the grayscale face; "smiling" is the softmax probability of
    /// its happiness class. The network expects raw 0..255 values.
    /// </summary>
    private double ComputeSmile(Mat aligned)
    {
        using var gray = new Mat();
        Cv2.CvtColor(aligned, gray, ColorConversionCodes.BGR2GRAY);
        using var blob = CvDnn.BlobFromImage(
            gray,
            1.0,
            new Size(EmotionInputSize, EmotionInputSize));
        emotionNet!.SetInput(blob);
        using var output = emotionNet.Forward();
        var logits = new float[output.Total()];
        for (var index = 0; index < logits.Length; index++)
        {
            logits[index] = output.At<float>(0, index);
        }

        return Softmax(logits)[HappinessClassIndex];
    }

    /// <summary>
    /// open-closed-eye-0001 on a patch around each aligned eye position;
    /// class 1 is "open". "Eyes open" means both are, so the face gets the
    /// weaker eye's probability.
    /// </summary>
    private double ComputeEyesOpen(Mat aligned)
    {
        var eyesOpen = 1.0;
        foreach (var eye in new[] { ReferenceLandmarks[0], ReferenceLandmarks[1] })
        {
            var left = Math.Clamp(
                (int)Math.Round(eye.X) - EyePatchSize / 2,
                0,
                AlignedSize - EyePatchSize);
            var top = Math.Clamp(
                (int)Math.Round(eye.Y) - EyePatchSize / 2,
                0,
                AlignedSize - EyePatchSize);
            using var patch = new Mat(
                aligned,
                new Rect(left, top, EyePatchSize, EyePatchSize));
            // The model's documented preprocessing: (BGR pixel - 127) / 255.
            using var blob = CvDnn.BlobFromImage(
                patch,
                1.0 / 255.0,
                new Size(EyePatchSize, EyePatchSize),
                new Scalar(127, 127, 127));
            eyeStateNet!.SetInput(blob);
            using var output = eyeStateNet.Forward();
            // The network normalizes internally: [closed, open] sum to one.
            var open = (double)output.At<float>(0, 1);
            eyesOpen = Math.Min(eyesOpen, open);
        }

        return eyesOpen;
    }

    private static double[] Softmax(IReadOnlyList<float> logits)
    {
        var max = logits.Max();
        var exponentials = logits.Select(value => Math.Exp(value - max)).ToArray();
        var sum = exponentials.Sum();
        return exponentials.Select(value => value / sum).ToArray();
    }

    private void EnsureLoaded()
    {
        if (detector is not null)
        {
            return;
        }

        if (!ModelsAvailable)
        {
            throw new FileNotFoundException(
                "The face models are missing; expected "
                + $"{DetectorModelPath} and {RecognizerModelPath}.");
        }

        detector = FaceDetectorYN.Create(
            DetectorModelPath,
            string.Empty,
            new Size(DetectionCanvasSize, DetectionCanvasSize),
            ScoreThreshold,
            NmsThreshold);
        recognizer = CvDnn.ReadNetFromOnnx(RecognizerModelPath);

        if (ExpressionModelsAvailable)
        {
            try
            {
                emotionNet = CvDnn.ReadNetFromOnnx(EmotionModelPath);
                eyeStateNet = CvDnn.ReadNetFromOnnx(EyeStateModelPath);
            }
            catch (Exception)
            {
                // A corrupt expression model must not take face detection
                // down with it; the scan simply runs without expressions.
                emotionNet?.Dispose();
                emotionNet = null;
                eyeStateNet = null;
            }
        }
    }

    /// <summary>
    /// The detector wants a 3-channel BGR frame; WPF bitmaps arrive in
    /// whatever format the decoder produced.
    /// </summary>
    private static Mat ToBgrMat(BitmapSource image)
    {
        var mat = image.ToMat();
        if (mat.Channels() == 3)
        {
            return mat;
        }

        var converted = new Mat();
        Cv2.CvtColor(
            mat,
            converted,
            mat.Channels() switch
            {
                4 => ColorConversionCodes.BGRA2BGR,
                1 => ColorConversionCodes.GRAY2BGR,
                _ => throw new NotSupportedException(
                    $"Unexpected channel count {mat.Channels()}.")
            });
        mat.Dispose();
        return converted;
    }

    public void Dispose()
    {
        lock (gate)
        {
            detector?.Dispose();
            recognizer?.Dispose();
            emotionNet?.Dispose();
            eyeStateNet?.Dispose();
            detector = null;
            recognizer = null;
            emotionNet = null;
            eyeStateNet = null;
        }
    }
}
