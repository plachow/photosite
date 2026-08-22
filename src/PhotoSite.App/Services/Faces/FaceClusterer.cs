using System.Windows;
using PhotoSite.Domain;

namespace PhotoSite.Services.Faces;

public static class FaceMath
{
    public static float[] Normalize(float[] vector)
    {
        double sum = 0;
        foreach (var value in vector)
        {
            sum += (double)value * value;
        }

        var length = Math.Sqrt(sum);
        if (length < 1e-12)
        {
            return vector;
        }

        var normalized = new float[vector.Length];
        for (var index = 0; index < vector.Length; index++)
        {
            normalized[index] = (float)(vector[index] / length);
        }

        return normalized;
    }

    /// <summary>Cosine similarity; 1 is identical, 0 unrelated.</summary>
    public static double Cosine(
        IReadOnlyList<float> first,
        IReadOnlyList<float> second)
    {
        if (first.Count != second.Count || first.Count == 0)
        {
            return 0;
        }

        double dot = 0;
        double firstNorm = 0;
        double secondNorm = 0;
        for (var index = 0; index < first.Count; index++)
        {
            dot += (double)first[index] * second[index];
            firstNorm += (double)first[index] * first[index];
            secondNorm += (double)second[index] * second[index];
        }

        var scale = Math.Sqrt(firstNorm) * Math.Sqrt(secondNorm);
        return scale < 1e-12 ? 0 : dot / scale;
    }
}

/// <summary>Unassigned faces the clusterer believes are one person.</summary>
public sealed record FaceCluster(
    IReadOnlyList<FaceRecord> Faces,
    float[] Centroid);

public static class FaceClusterer
{
    /// <summary>
    /// OpenCV's decision boundary for "same identity" with SFace embeddings
    /// is a cosine of 0.363; grouping strangers together is worse than
    /// splitting one person into two groups, so clustering stays well above
    /// it.
    /// </summary>
    public const double GroupingThreshold = 0.45;

    /// <summary>
    /// Automatically attaching a new face to an already-named person writes
    /// that name into a file, so it demands more certainty still.
    /// </summary>
    public const double AutoMatchThreshold = 0.5;

    /// <summary>
    /// OpenCV's same-identity boundary for SFace. A match above it that does
    /// not reach <see cref="AutoMatchThreshold"/> becomes a suggestion the
    /// user confirms or rejects instead of a silent assignment.
    /// </summary>
    public const double SuggestThreshold = 0.363;

    /// <summary>
    /// Greedy centroid clustering: every face joins the most similar
    /// existing group above the threshold, or founds a new one. Confident
    /// detections go first so the seeds are the sharpest faces.
    /// </summary>
    public static IReadOnlyList<FaceCluster> Cluster(
        IReadOnlyList<FaceRecord> faces,
        double threshold = GroupingThreshold)
    {
        var clusters = new List<(List<FaceRecord> Members, double[] Sum)>();
        foreach (var face in faces.OrderByDescending(item => item.Confidence))
        {
            var bestIndex = -1;
            var bestSimilarity = threshold;
            for (var index = 0; index < clusters.Count; index++)
            {
                var similarity = FaceMath.Cosine(
                    face.Embedding,
                    ToCentroid(clusters[index].Sum));
                if (similarity >= bestSimilarity)
                {
                    bestSimilarity = similarity;
                    bestIndex = index;
                }
            }

            if (bestIndex < 0)
            {
                var sum = new double[face.Embedding.Length];
                Accumulate(sum, face.Embedding);
                clusters.Add(([face], sum));
            }
            else
            {
                clusters[bestIndex].Members.Add(face);
                Accumulate(clusters[bestIndex].Sum, face.Embedding);
            }
        }

        return clusters
            .OrderByDescending(cluster => cluster.Members.Count)
            .Select(cluster => new FaceCluster(
                cluster.Members,
                ToCentroid(cluster.Sum)))
            .ToArray();
    }

    /// <summary>
    /// The average embedding of one person's faces, for matching newly
    /// scanned faces against people who already have a name.
    /// </summary>
    public static float[] Centroid(IReadOnlyList<float[]> embeddings)
    {
        if (embeddings.Count == 0)
        {
            return [];
        }

        var sum = new double[embeddings[0].Length];
        foreach (var embedding in embeddings)
        {
            Accumulate(sum, embedding);
        }

        return ToCentroid(sum);
    }

    private static void Accumulate(double[] sum, IReadOnlyList<float> vector)
    {
        for (var index = 0; index < sum.Length; index++)
        {
            sum[index] += vector[index];
        }
    }

    private static float[] ToCentroid(double[] sum)
    {
        var vector = new float[sum.Length];
        for (var index = 0; index < sum.Length; index++)
        {
            vector[index] = (float)sum[index];
        }

        return FaceMath.Normalize(vector);
    }
}

public static class FaceCropper
{
    /// <summary>
    /// The pixel rectangle to cut a face thumbnail out of a decoded preview:
    /// the detector's box grown by a margin so hair and chin survive, then
    /// clamped to the frame.
    /// </summary>
    public static Int32Rect ComputeCropRect(
        int pixelWidth,
        int pixelHeight,
        FaceRecord face,
        double margin = 0.35)
    {
        var x = (face.X - face.Width * margin) * pixelWidth;
        var y = (face.Y - face.Height * margin) * pixelHeight;
        var width = face.Width * (1 + 2 * margin) * pixelWidth;
        var height = face.Height * (1 + 2 * margin) * pixelHeight;

        var left = Math.Clamp((int)Math.Round(x), 0, Math.Max(0, pixelWidth - 1));
        var top = Math.Clamp((int)Math.Round(y), 0, Math.Max(0, pixelHeight - 1));
        var right = Math.Clamp((int)Math.Round(x + width), left + 1, pixelWidth);
        var bottom = Math.Clamp((int)Math.Round(y + height), top + 1, pixelHeight);
        return new Int32Rect(left, top, right - left, bottom - top);
    }
}
