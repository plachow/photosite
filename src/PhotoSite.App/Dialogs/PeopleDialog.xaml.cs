using System.Windows;
using System.Windows.Media;
using System.Windows.Media.Imaging;
using PhotoSite.Domain;
using PhotoSite.Infrastructure;
using PhotoSite.Services;
using PhotoSite.Services.Faces;

namespace PhotoSite.Dialogs;

/// <summary>
/// Stage one of face recognition: scans the current folder with the local
/// YuNet + SFace models, groups the unnamed faces, and lets whole groups be
/// named at once. A name becomes a person in the catalogue and a keyword on
/// every photograph the person appears in; newly scanned faces that clearly
/// match an already-named person are assigned automatically.
/// </summary>
public partial class PeopleDialog : Window
{
    private const int DetectPixelWidth = 1024;
    private const int ThumbnailPixelWidth = 512;
    private const int ThumbnailsPerCluster = 5;
    private const int MaxClustersShown = 60;

    private readonly IReadOnlyList<PhotoRecord> photos;
    private readonly FaceEngine engine;
    private readonly PhotoCatalogRepository catalog;
    private readonly PreviewService previews;
    private CancellationTokenSource? scanCancellation;
    private bool isScanning;

    internal PeopleDialog(
        IReadOnlyList<PhotoRecord> photos,
        FaceEngine engine,
        PhotoCatalogRepository catalog,
        PreviewService previews)
    {
        this.photos = photos;
        this.engine = engine;
        this.catalog = catalog;
        this.previews = previews;
        InitializeComponent();
        Loaded += async (_, _) => await RefreshAsync();
    }

    private sealed class ClusterRow
    {
        public required IReadOnlyList<ImageSource> Thumbnails { get; init; }

        public required string CountText { get; init; }

        public required IReadOnlyList<long> FaceIds { get; init; }

        public required IReadOnlyList<string> Paths { get; init; }

        public required IReadOnlyList<string> KnownNames { get; init; }

        public string Name { get; set; } = string.Empty;
    }

    private sealed record PersonRow(PersonRecord Person)
    {
        public override string ToString() =>
            $"{Person.Name} · {Person.FaceCount:N0} faces";
    }

    private async Task RefreshAsync()
    {
        try
        {
            var people = await catalog.GetPeopleAsync();
            PeopleList.ItemsSource = people
                .Select(person => new PersonRow(person))
                .ToArray();
            var knownNames = people.Select(person => person.Name).ToArray();

            var unassigned = await catalog.GetUnassignedFacesAsync();
            var clusters = FaceClusterer.Cluster(unassigned);
            var groups = clusters
                .Where(cluster => cluster.Faces.Count >= 2)
                .Take(MaxClustersShown)
                .ToArray();
            var singles = unassigned.Count
                          - groups.Sum(cluster => cluster.Faces.Count);

            var rows = new List<ClusterRow>(groups.Length);
            var previewCache = new Dictionary<string, BitmapSource?>(
                StringComparer.OrdinalIgnoreCase);
            foreach (var cluster in groups)
            {
                rows.Add(new ClusterRow
                {
                    Thumbnails = await LoadClusterThumbnailsAsync(
                        cluster,
                        previewCache),
                    CountText = cluster.Faces.Count == 1
                        ? "1 face"
                        : $"{cluster.Faces.Count:N0} faces in "
                          + $"{cluster.Faces.Select(face => face.Path).Distinct(StringComparer.OrdinalIgnoreCase).Count():N0} photos",
                    FaceIds = cluster.Faces.Select(face => face.Id).ToArray(),
                    Paths = cluster.Faces
                        .Select(face => face.Path)
                        .Distinct(StringComparer.OrdinalIgnoreCase)
                        .ToArray(),
                    KnownNames = knownNames
                });
            }

            ClusterList.ItemsSource = rows;

            if (!isScanning)
            {
                var parts = new List<string>();
                if (!FaceEngine.ModelsAvailable)
                {
                    ScanButton.IsEnabled = false;
                    parts.Add(
                        "The face models were not found in "
                        + FaceEngine.ModelDirectoryPath);
                }
                else
                {
                    parts.Add($"{photos.Count:N0} photos in this folder");
                }

                if (rows.Count > 0)
                {
                    parts.Add($"{rows.Count:N0} unnamed groups");
                }

                if (singles > 0)
                {
                    parts.Add($"{singles:N0} faces without a group yet");
                }

                SummaryText.Text = string.Join(" · ", parts);
            }
        }
        catch (Exception exception)
        {
            SummaryText.Text =
                $"The face catalogue could not be loaded: {exception.Message}";
        }
    }

    private async Task<IReadOnlyList<ImageSource>> LoadClusterThumbnailsAsync(
        FaceCluster cluster,
        Dictionary<string, BitmapSource?> previewCache)
    {
        var thumbnails = new List<ImageSource>(ThumbnailsPerCluster);
        foreach (var face in cluster.Faces.Take(ThumbnailsPerCluster))
        {
            if (!previewCache.TryGetValue(face.Path, out var preview))
            {
                try
                {
                    preview = File.Exists(face.Path)
                        ? await previews.LoadAsync(
                            face.Path,
                            ThumbnailPixelWidth,
                            CancellationToken.None)
                        : null;
                }
                catch (Exception)
                {
                    // A face row can outlive its file or codec; the group
                    // is still nameable without this thumbnail.
                    preview = null;
                }

                previewCache[face.Path] = preview;
            }

            if (preview is null)
            {
                continue;
            }

            var crop = new CroppedBitmap(
                preview,
                FaceCropper.ComputeCropRect(
                    preview.PixelWidth,
                    preview.PixelHeight,
                    face));
            crop.Freeze();
            thumbnails.Add(crop);
        }

        return thumbnails;
    }

    private async void OnScanClick(object sender, RoutedEventArgs eventArgs)
    {
        if (isScanning)
        {
            scanCancellation?.Cancel();
            return;
        }

        scanCancellation = new CancellationTokenSource();
        var token = scanCancellation.Token;
        SetScanningState(true);
        var scanned = 0;
        var skipped = 0;
        var facesFound = 0;
        var autoAssigned = 0;
        var failures = 0;

        try
        {
            var states = await catalog.GetFaceScanStatesAsync(token);
            var centroids = await LoadPersonCentroidsAsync(token);

            for (var index = 0; index < photos.Count; index++)
            {
                token.ThrowIfCancellationRequested();
                var photo = photos[index];
                ScanProgress.Value = 100d * index / photos.Count;
                SummaryText.Text =
                    $"{index + 1:N0} / {photos.Count:N0} · {photo.FileName}"
                    + $" · {facesFound:N0} faces";

                if (states.TryGetValue(photo.Path, out var seenTicks)
                    && seenTicks == photo.ModifiedUtcTicks)
                {
                    skipped++;
                    continue;
                }

                if (!File.Exists(photo.Path))
                {
                    skipped++;
                    continue;
                }

                try
                {
                    var preview = await previews.LoadAsync(
                        photo.Path,
                        DetectPixelWidth,
                        token);
                    var detected = await Task.Run(
                        () => engine.Detect(preview),
                        token);

                    var rows = new List<(double, double, double, double,
                        double, float[], long?)>(detected.Count);
                    var namesForPhoto = new List<string>();
                    foreach (var face in detected)
                    {
                        var match = MatchPerson(face.Embedding, centroids);
                        if (match is { } person)
                        {
                            autoAssigned++;
                            namesForPhoto.Add(person.Name);
                        }

                        rows.Add((
                            face.X,
                            face.Y,
                            face.Width,
                            face.Height,
                            face.Confidence,
                            face.Embedding,
                            match?.Id));
                    }

                    await catalog.ReplaceFacesAsync(
                        photo.Path,
                        photo.ModifiedUtcTicks,
                        rows,
                        token);
                    foreach (var name in namesForPhoto.Distinct(
                                 StringComparer.OrdinalIgnoreCase))
                    {
                        await WritePersonKeywordAsync(photo.Path, name);
                    }

                    facesFound += detected.Count;
                    scanned++;
                }
                catch (OperationCanceledException)
                    when (token.IsCancellationRequested)
                {
                    throw;
                }
                catch (Exception)
                {
                    // An undecodable or vanished file must not end the sweep.
                    failures++;
                }
            }
        }
        catch (OperationCanceledException)
        {
            DetailText.Text = "Stopped; everything scanned so far is kept.";
        }
        catch (Exception exception)
        {
            DetailText.Text = $"The scan failed: {exception.Message}";
        }
        finally
        {
            scanCancellation?.Dispose();
            scanCancellation = null;
            SetScanningState(false);
        }

        var summary = new List<string>
        {
            $"Scanned {scanned:N0} photo(s)",
            $"{facesFound:N0} faces"
        };
        if (autoAssigned > 0)
        {
            summary.Add($"{autoAssigned:N0} matched to known people");
        }

        if (skipped > 0)
        {
            summary.Add($"{skipped:N0} already scanned");
        }

        if (failures > 0)
        {
            summary.Add($"{failures:N0} failed");
        }

        SummaryText.Text = string.Join(" · ", summary);
        await RefreshAsync();
    }

    private sealed record PersonCentroid(long Id, string Name, float[] Centroid);

    private async Task<IReadOnlyList<PersonCentroid>> LoadPersonCentroidsAsync(
        CancellationToken cancellationToken)
    {
        var people = await catalog.GetPeopleAsync(cancellationToken);
        var assigned = await catalog.GetAssignedFacesAsync(cancellationToken);
        var byPerson = assigned
            .Where(face => face.PersonId is not null)
            .GroupBy(face => face.PersonId!.Value)
            .ToDictionary(
                group => group.Key,
                group => FaceClusterer.Centroid(
                    group.Select(face => face.Embedding).ToArray()));
        return people
            .Where(person => byPerson.ContainsKey(person.Id))
            .Select(person => new PersonCentroid(
                person.Id,
                person.Name,
                byPerson[person.Id]))
            .ToArray();
    }

    private static PersonCentroid? MatchPerson(
        float[] embedding,
        IReadOnlyList<PersonCentroid> centroids)
    {
        PersonCentroid? best = null;
        var bestSimilarity = FaceClusterer.AutoMatchThreshold;
        foreach (var candidate in centroids)
        {
            var similarity = FaceMath.Cosine(embedding, candidate.Centroid);
            if (similarity >= bestSimilarity)
            {
                bestSimilarity = similarity;
                best = candidate;
            }
        }

        return best;
    }

    /// <summary>
    /// Adds the person's name to the photograph's keywords through the
    /// ordinary catalogue path, so the outbox writes it into the file.
    /// </summary>
    private async Task WritePersonKeywordAsync(string path, string name)
    {
        var record = await catalog.GetByPathAsync(path);
        if (record is null)
        {
            return;
        }

        var merged = OllamaVisionService.MergeKeywords(record.Keywords, [name]);
        if (merged is not null
            && !string.Equals(merged, record.Keywords, StringComparison.Ordinal))
        {
            await catalog.UpdateKeywordsAsync(path, merged);
        }
    }

    private async void OnAssignClusterClick(
        object sender,
        RoutedEventArgs eventArgs)
    {
        if (sender is not FrameworkElement { Tag: ClusterRow row })
        {
            return;
        }

        var name = row.Name.Trim();
        if (name.Length == 0)
        {
            DetailText.Text = "Type or pick a name for the group first.";
            return;
        }

        try
        {
            var personId = await catalog.GetOrCreatePersonAsync(name);
            await catalog.AssignFacesAsync(row.FaceIds, personId);
            foreach (var path in row.Paths)
            {
                await WritePersonKeywordAsync(path, name);
            }

            DetailText.Text =
                $"Named {row.FaceIds.Count:N0} face(s) as “{name}” and wrote "
                + $"the keyword into {row.Paths.Count:N0} photo(s).";
        }
        catch (Exception exception)
        {
            DetailText.Text = $"Naming the group failed: {exception.Message}";
        }

        await RefreshAsync();
    }

    private async void OnRenamePersonClick(
        object sender,
        RoutedEventArgs eventArgs)
    {
        if (PeopleList.SelectedItem is not PersonRow row)
        {
            DetailText.Text = "Select a person to rename first.";
            return;
        }

        var dialog = new TextPromptDialog(
            "Rename person",
            "Name",
            row.Person.Name)
        {
            Owner = this
        };
        if (dialog.ShowDialog() != true
            || string.IsNullOrWhiteSpace(dialog.Value))
        {
            return;
        }

        var name = dialog.Value.Trim();
        try
        {
            await catalog.RenamePersonAsync(row.Person.Id, name);
            // The new name joins the keywords of the person's photos; the
            // old keyword is left in place rather than silently rewritten.
            var faces = await catalog.GetFacesForPersonAsync(row.Person.Id);
            foreach (var path in faces
                         .Select(face => face.Path)
                         .Distinct(StringComparer.OrdinalIgnoreCase))
            {
                await WritePersonKeywordAsync(path, name);
            }
        }
        catch (Exception exception)
        {
            DetailText.Text = $"The rename failed: {exception.Message}";
        }

        await RefreshAsync();
    }

    private async void OnDeletePersonClick(
        object sender,
        RoutedEventArgs eventArgs)
    {
        if (PeopleList.SelectedItem is not PersonRow row)
        {
            DetailText.Text = "Select a person to remove first.";
            return;
        }

        if (MessageBox.Show(
                this,
                $"Remove “{row.Person.Name}”?\n\nTheir faces return to the "
                + "unnamed groups. Keywords already written into the photo "
                + "files are not touched.",
                "Remove person",
                MessageBoxButton.YesNo,
                MessageBoxImage.Question,
                MessageBoxResult.No) != MessageBoxResult.Yes)
        {
            return;
        }

        try
        {
            await catalog.DeletePersonAsync(row.Person.Id);
        }
        catch (Exception exception)
        {
            DetailText.Text = $"The removal failed: {exception.Message}";
        }

        await RefreshAsync();
    }

    private void SetScanningState(bool scanning)
    {
        isScanning = scanning;
        ScanProgress.Visibility = scanning
            ? Visibility.Visible
            : Visibility.Collapsed;
        ScanProgress.Value = 0;
        ScanButton.Content = scanning ? "Stop" : "Scan folder for faces";
        CloseButton.IsEnabled = !scanning;
        if (!scanning)
        {
            ScanButton.IsEnabled = FaceEngine.ModelsAvailable;
        }
    }

    private void OnCloseClick(object sender, RoutedEventArgs eventArgs) =>
        Close();

    protected override void OnClosing(System.ComponentModel.CancelEventArgs e)
    {
        if (isScanning)
        {
            // The sweep finishes the photograph in flight and stops; every
            // face already stored stays stored.
            e.Cancel = true;
            scanCancellation?.Cancel();
            return;
        }

        base.OnClosing(e);
    }
}
