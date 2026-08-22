using System.Windows.Media.Imaging;
using OpenCvSharp;
using OpenCvSharp.Dnn;
using OpenCvSharp.WpfExtensions;

namespace PhotoSite.Services.Faces;

/// <summary>One face found in an image, in normalized 0..1 coordinates.</summary>
public sealed record DetectedFace(
    double X,
    double Y,
    double Width,
    double Height,
    double Confidence,
    float[] Embedding);

/// <summary>
/// Local face detection and recognition: YuNet finds the faces and their
/// five landmarks, SFace turns each aligned face into a 128-dimensional
/// embedding whose cosine similarity says whether two faces belong to the
/// same person. Both models come from the OpenCV Zoo (Apache-2.0) and
/// everything runs on this machine.
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

    private readonly object gate = new();
    private FaceDetectorYN? detector;
    private Net? recognizer;

    public static bool ModelsAvailable =>
        File.Exists(DetectorModelPath) && File.Exists(RecognizerModelPath);

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

                if (ComputeEmbedding(frame, landmarks) is not { } embedding)
                {
                    continue;
                }

                results.Add(new DetectedFace(
                    Math.Clamp(x / scale / frameWidth, 0, 1),
                    Math.Clamp(y / scale / frameHeight, 0, 1),
                    Math.Clamp(width / scale / frameWidth, 0, 1),
                    Math.Clamp(height / scale / frameHeight, 0, 1),
                    score,
                    FaceMath.Normalize(embedding)));
            }

            return results;
        }
    }

    /// <summary>
    /// Warps the face so its landmarks land on the reference positions -
    /// the same alignment OpenCV's FaceRecognizerSF performs - and runs the
    /// SFace network on the 112×112 crop.
    /// </summary>
    private float[]? ComputeEmbedding(Mat frame, Point2f[] landmarks)
    {
        using var transform = Cv2.EstimateAffinePartial2D(
            InputArray.Create(landmarks),
            InputArray.Create(ReferenceLandmarks));
        if (transform is null || transform.Empty())
        {
            return null;
        }

        using var aligned = new Mat();
        Cv2.WarpAffine(
            frame,
            aligned,
            transform,
            new Size(AlignedSize, AlignedSize));

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
            detector = null;
            recognizer = null;
        }
    }
}
