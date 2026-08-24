using System.Windows;
using System.Windows.Media;
using System.Windows.Media.Imaging;
using PhotoSite.Domain;
using PhotoSite.Infrastructure;
using PhotoSite.Services;
using PhotoSite.Services.Faces;

namespace PhotoSite.Dialogs;

/// <summary>
/// Stage one of face recognition: scans the given photographs - the gallery
/// selection, or the whole folder when nothing is selected - with the local
/// YuNet + SFace models, groups the unnamed faces, and lets whole groups be
/// named at once. A name becomes a person in the catalogue and a keyword on
/// every photograph the person appears in; newly scanned faces that clearly
/// match an already-named person are assigned automatically, and a group of
/// strangers can be waved away for good.
/// </summary>
public partial class PeopleDialog : Window
{
    private const int DetectPixelWidth = 1024;
    private const int ThumbnailPixelWidth = 512;
    private const int ThumbnailsPerCluster = 5;
    private const int MaxClustersShown = 60;
    private const int MaxPersonFacesShown = 60;

    private readonly IReadOnlyList<PhotoRecord> photos;
    private readonly FaceEngine engine;
    private readonly PhotoCatalogRepository catalog;
    private readonly PreviewService previews;
    private readonly bool scanningSelection;
    private CancellationTokenSource? scanCancellation;
    private bool isScanning;

    internal PeopleDialog(
        IReadOnlyList<PhotoRecord> photos,
        FaceEngine engine,
        PhotoCatalogRepository catalog,
        PreviewService previews,
        bool scanningSelection)
    {
        this.photos = photos;
        this.engine = engine;
        this.catalog = catalog;
        this.previews = previews;
        this.scanningSelection = scanningSelection;
        InitializeComponent();
        DarkWindowChrome.Apply(this);
        ScanButton.Content = ScanButtonLabel;
        Loaded += async (_, _) => await RefreshAsync();
    }

    private string ScanButtonLabel => scanningSelection
        ? "Scan selection for faces"
        : "Scan folder for faces";

    private sealed class ClusterRow
    {
        public required IReadOnlyList<ImageSource> Thumbnails { get; init; }

        public required string CountText { get; init; }

        public required IReadOnlyList<long> FaceIds { get; init; }

        public required IReadOnlyList<string> Paths { get; init; }

        // Filled right after construction; each chip needs its owning row.
        public IReadOnlyList<AssignChoice> AssignChoices { get; set; } = [];
    }

    /// <summary>One chip on a group card: this group is that person.</summary>
    private sealed record AssignChoice(ClusterRow Cluster, PersonRecord Person);

    private sealed class PersonFaceRow
    {
        public required ImageSource? Thumbnail { get; init; }

        public required FaceRecord Face { get; init; }
    }

    private sealed record PersonRow(PersonRecord Person)
    {
        public override string ToString() =>
            $"{Person.Name} · {Person.FaceCount:N0} faces";
    }

    private sealed class SuggestionRow
    {
        public required IReadOnlyList<ImageSource> Thumbnails { get; init; }

        public required string QuestionText { get; init; }

        public required IReadOnlyList<long> FaceIds { get; init; }

        public required IReadOnlyList<string> Paths { get; init; }

        public required long PersonId { get; init; }

        public required string PersonName { get; init; }
    }

    private async Task RefreshAsync()
    {
        try
        {
            var people = await catalog.GetPeopleAsync();
            var selectedPersonId =
                (PeopleList.SelectedItem as PersonRow)?.Person.Id;
            var personRows = people
                .Select(person => new PersonRow(person))
                .ToArray();
            PeopleList.ItemsSource = personRows;
            PeopleList.SelectedItem = personRows.FirstOrDefault(
                row => row.Person.Id == selectedPersonId);

            var previewCache = new Dictionary<string, BitmapSource?>(
                StringComparer.OrdinalIgnoreCase);

            // Borderline matches first: each suggested person gets one card
            // with a plain yes or no.
            var suggested = await catalog.GetSuggestedFacesAsync();
            var peopleById = people.ToDictionary(
                person => person.Id,
                person => person.Name);
            var suggestionRows = new List<SuggestionRow>();
            foreach (var group in suggested
                         .Where(face => face.SuggestedPersonId is { } id
                                        && peopleById.ContainsKey(id))
                         .GroupBy(face => face.SuggestedPersonId!.Value))
            {
                var faces = group.ToArray();
                var name = peopleById[group.Key];
                suggestionRows.Add(new SuggestionRow
                {
                    Thumbnails = await LoadFaceThumbnailsAsync(
                        faces,
                        previewCache),
                    QuestionText = faces.Length == 1
                        ? $"Is this {name}?"
                        : $"Are these {faces.Length:N0} faces {name}?",
                    FaceIds = faces.Select(face => face.Id).ToArray(),
                    Paths = faces
                        .Select(face => face.Path)
                        .Distinct(StringComparer.OrdinalIgnoreCase)
                        .ToArray(),
                    PersonId = group.Key,
                    PersonName = name
                });
            }

            SuggestionList.ItemsSource = suggestionRows;
            SuggestionsHeader.Visibility = suggestionRows.Count > 0
                ? Visibility.Visible
                : Visibility.Collapsed;

            var unassigned = await catalog.GetUnassignedFacesAsync();
            var clusters = FaceClusterer.Cluster(unassigned);
            var groups = clusters
                .Where(cluster => cluster.Faces.Count >= 2)
                .Take(MaxClustersShown)
                .ToArray();
            var singles = unassigned.Count
                          - groups.Sum(cluster => cluster.Faces.Count);

            var rows = new List<ClusterRow>(groups.Length);
            foreach (var cluster in groups)
            {
                var row = new ClusterRow
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
                        .ToArray()
                };
                row.AssignChoices = people
                    .Select(person => new AssignChoice(row, person))
                    .ToArray();
                rows.Add(row);
            }

            ClusterList.ItemsSource = rows;
            await LoadSelectedPersonFacesAsync();

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
                    parts.Add(scanningSelection
                        ? $"{photos.Count:N0} selected photo(s)"
                        : $"{photos.Count:N0} photos in this folder");
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

    private Task<IReadOnlyList<ImageSource>> LoadClusterThumbnailsAsync(
        FaceCluster cluster,
        Dictionary<string, BitmapSource?> previewCache) =>
        LoadFaceThumbnailsAsync(cluster.Faces, previewCache);

    private async Task<IReadOnlyList<ImageSource>> LoadFaceThumbnailsAsync(
        IReadOnlyList<FaceRecord> faces,
        Dictionary<string, BitmapSource?> previewCache)
    {
        var thumbnails = new List<ImageSource>(ThumbnailsPerCluster);
        foreach (var face in faces.Take(ThumbnailsPerCluster))
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
        var suggestionsFound = 0;
        var failures = 0;
        var expressionsScored = 0;

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

                    var rows = new List<FaceObservation>(detected.Count);
                    var namesForPhoto = new List<string>();
                    foreach (var face in detected)
                    {
                        var (match, similarity) = FindBestPerson(
                            face.Embedding,
                            centroids);
                        long? assignedId = null;
                        long? suggestedId = null;
                        if (match is { } person)
                        {
                            if (similarity >= FaceClusterer.AutoMatchThreshold)
                            {
                                assignedId = person.Id;
                                autoAssigned++;
                                namesForPhoto.Add(person.Name);
                            }
                            else
                            {
                                // Probably them, but not certainly enough to
                                // write a name into a file unasked.
                                suggestedId = person.Id;
                                suggestionsFound++;
                            }
                        }

                        rows.Add(new FaceObservation(
                            face.X,
                            face.Y,
                            face.Width,
                            face.Height,
                            face.Confidence,
                            face.Embedding,
                            assignedId,
                            suggestedId,
                            face.Smile,
                            face.EyesOpen));
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

                    if (namesForPhoto.Count > 0)
                    {
                        await WriteFaceRegionsAsync(photo.Path);
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

            expressionsScored = await BackfillExpressionsAsync(token);
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

        if (suggestionsFound > 0)
        {
            summary.Add($"{suggestionsFound:N0} to confirm below");
        }

        if (expressionsScored > 0)
        {
            summary.Add(
                $"{expressionsScored:N0} older face(s) scored for expression");
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

    /// <summary>
    /// Faces scanned before the expression models existed carry no smile or
    /// eyes score. This pass re-detects those photographs and copies the
    /// scores onto the stored faces by rectangle overlap, leaving their ids,
    /// names and suggestions untouched.
    /// </summary>
    private async Task<int> BackfillExpressionsAsync(CancellationToken token)
    {
        if (!FaceEngine.ExpressionModelsAvailable)
        {
            return 0;
        }

        var folderPaths = photos
            .Select(photo => photo.Path)
            .ToHashSet(StringComparer.OrdinalIgnoreCase);
        var pending = (await catalog.GetPathsMissingExpressionsAsync(token))
            .Where(folderPaths.Contains)
            .Where(File.Exists)
            .ToArray();
        var scored = 0;
        for (var index = 0; index < pending.Length; index++)
        {
            token.ThrowIfCancellationRequested();
            var path = pending[index];
            ScanProgress.Value = 100d * index / pending.Length;
            SummaryText.Text =
                $"Expressions · {index + 1:N0} / {pending.Length:N0}"
                + $" · {Path.GetFileName(path)}";
            try
            {
                var preview = await previews.LoadAsync(
                    path,
                    DetectPixelWidth,
                    token);
                var detected = await Task.Run(() => engine.Detect(preview), token);
                var stored = await catalog.GetFacesForPathAsync(path);
                var updates = new List<(long, double?, double?)>();
                foreach (var face in stored.Where(face =>
                             face.Smile is null || face.EyesOpen is null))
                {
                    var match = detected
                        .Where(candidate => Overlap(face, candidate) >= 0.5)
                        .OrderByDescending(candidate => Overlap(face, candidate))
                        .FirstOrDefault();
                    if (match is { Smile: not null } or { EyesOpen: not null })
                    {
                        updates.Add((face.Id, match!.Smile, match.EyesOpen));
                    }
                }

                await catalog.UpdateFaceExpressionsAsync(updates, token);
                scored += updates.Count;
            }
            catch (OperationCanceledException)
                when (token.IsCancellationRequested)
            {
                throw;
            }
            catch (Exception)
            {
                // The photo keeps its unscored faces; the next scan retries.
            }
        }

        return scored;
    }

    /// <summary>Intersection over union of two normalized face rectangles.</summary>
    private static double Overlap(FaceRecord stored, DetectedFace detected)
    {
        var left = Math.Max(stored.X, detected.X);
        var top = Math.Max(stored.Y, detected.Y);
        var right = Math.Min(stored.X + stored.Width, detected.X + detected.Width);
        var bottom = Math.Min(
            stored.Y + stored.Height,
            detected.Y + detected.Height);
        if (right <= left || bottom <= top)
        {
            return 0;
        }

        var intersection = (right - left) * (bottom - top);
        var union = stored.Width * stored.Height
                    + detected.Width * detected.Height
                    - intersection;
        return union <= 0 ? 0 : intersection / union;
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

    private static (PersonCentroid? Match, double Similarity) FindBestPerson(
        float[] embedding,
        IReadOnlyList<PersonCentroid> centroids)
    {
        PersonCentroid? best = null;
        var bestSimilarity = FaceClusterer.SuggestThreshold;
        foreach (var candidate in centroids)
        {
            var similarity = FaceMath.Cosine(embedding, candidate.Centroid);
            if (similarity >= bestSimilarity)
            {
                bestSimilarity = similarity;
                best = candidate;
            }
        }

        return (best, best is null ? 0 : bestSimilarity);
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

    /// <summary>
    /// Queues the photograph's named face rectangles as MWG regions, the
    /// face-frame format Lightroom, digiKam and Windows understand. Needs
    /// the pixel dimensions from the catalogue; without them the write is
    /// skipped rather than guessed.
    /// </summary>
    private async Task WriteFaceRegionsAsync(string path)
    {
        var record = await catalog.GetByPathAsync(path);
        if (record is not
            {
                PixelWidth: > 0 and { } width,
                PixelHeight: > 0 and { } height
            })
        {
            return;
        }

        var faces = await catalog.GetFacesForPathAsync(path);
        var names = (await catalog.GetPeopleAsync()).ToDictionary(
            person => person.Id,
            person => person.Name);
        var regions = faces
            .Where(face => face.PersonId is { } id && names.ContainsKey(id))
            .Select(face => new
            {
                name = names[face.PersonId!.Value],
                x = face.X,
                y = face.Y,
                w = face.Width,
                h = face.Height
            })
            .ToArray();
        await catalog.EnqueueFaceRegionsAsync(
            path,
            System.Text.Json.JsonSerializer.Serialize(
                new { width, height, regions }));
    }

    private async void OnConfirmSuggestionClick(
        object sender,
        RoutedEventArgs eventArgs)
    {
        if (sender is not FrameworkElement { Tag: SuggestionRow row })
        {
            return;
        }

        try
        {
            await catalog.AssignFacesAsync(row.FaceIds, row.PersonId);
            foreach (var path in row.Paths)
            {
                await WritePersonKeywordAsync(path, row.PersonName);
                await WriteFaceRegionsAsync(path);
            }

            DetailText.Text =
                $"Confirmed {row.FaceIds.Count:N0} face(s) as "
                + $"“{row.PersonName}”.";
        }
        catch (Exception exception)
        {
            DetailText.Text = $"Confirming failed: {exception.Message}";
        }

        await RefreshAsync();
    }

    private async void OnRejectSuggestionClick(
        object sender,
        RoutedEventArgs eventArgs)
    {
        if (sender is not FrameworkElement { Tag: SuggestionRow row })
        {
            return;
        }

        try
        {
            await catalog.ClearSuggestionsAsync(row.FaceIds);
            DetailText.Text =
                $"{row.FaceIds.Count:N0} face(s) returned to the unnamed "
                + "groups.";
        }
        catch (Exception exception)
        {
            DetailText.Text = $"Rejecting failed: {exception.Message}";
        }

        await RefreshAsync();
    }

    private async void OnAssignChipClick(
        object sender,
        RoutedEventArgs eventArgs)
    {
        if (sender is FrameworkElement { Tag: AssignChoice choice })
        {
            await AssignClusterAsync(choice.Cluster, choice.Person.Name);
        }
    }

    private async void OnAssignNewPersonClick(
        object sender,
        RoutedEventArgs eventArgs)
    {
        if (sender is not FrameworkElement { Tag: ClusterRow row })
        {
            return;
        }

        var dialog = new TextPromptDialog("New person", "Name", string.Empty)
        {
            Owner = this
        };
        if (dialog.ShowDialog() == true
            && !string.IsNullOrWhiteSpace(dialog.Value))
        {
            await AssignClusterAsync(row, dialog.Value.Trim());
        }
    }

    private async void OnIgnoreClusterClick(
        object sender,
        RoutedEventArgs eventArgs)
    {
        if (sender is not FrameworkElement { Tag: ClusterRow row })
        {
            return;
        }

        try
        {
            await catalog.IgnoreFacesAsync(row.FaceIds);
            DetailText.Text =
                $"Ignored {row.FaceIds.Count:N0} face(s); the group will "
                + "not be offered again.";
        }
        catch (Exception exception)
        {
            DetailText.Text = $"Ignoring the group failed: {exception.Message}";
        }

        await RefreshAsync();
    }

    private async Task AssignClusterAsync(ClusterRow row, string name)
    {
        try
        {
            var personId = await catalog.GetOrCreatePersonAsync(name);
            await catalog.AssignFacesAsync(row.FaceIds, personId);
            foreach (var path in row.Paths)
            {
                await WritePersonKeywordAsync(path, name);
                await WriteFaceRegionsAsync(path);
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

    private async void OnPeopleSelectionChanged(
        object sender,
        System.Windows.Controls.SelectionChangedEventArgs eventArgs) =>
        await LoadSelectedPersonFacesAsync();

    /// <summary>
    /// Shows the selected person's faces for corrective editing; a wrongly
    /// matched face is removed with its ✕.
    /// </summary>
    private async Task LoadSelectedPersonFacesAsync()
    {
        if (PeopleList.SelectedItem is not PersonRow row)
        {
            PersonFacesHeader.Visibility = Visibility.Collapsed;
            PersonFacesList.ItemsSource = null;
            return;
        }

        try
        {
            var faces = (await catalog.GetFacesForPersonAsync(row.Person.Id))
                .Take(MaxPersonFacesShown)
                .ToArray();
            var previewCache = new Dictionary<string, BitmapSource?>(
                StringComparer.OrdinalIgnoreCase);
            var faceRows = new List<PersonFaceRow>(faces.Length);
            foreach (var face in faces)
            {
                var thumbnails = await LoadFaceThumbnailsAsync(
                    [face],
                    previewCache);
                faceRows.Add(new PersonFaceRow
                {
                    Thumbnail = thumbnails.FirstOrDefault(),
                    Face = face
                });
            }

            PersonFacesHeader.Text =
                $"FACES OF {row.Person.Name.ToUpperInvariant()}"
                + (row.Person.FaceCount > faces.Length
                    ? $" (FIRST {faces.Length:N0} OF {row.Person.FaceCount:N0})"
                    : string.Empty);
            PersonFacesHeader.Visibility = Visibility.Visible;
            PersonFacesList.ItemsSource = faceRows;
        }
        catch (Exception exception)
        {
            DetailText.Text =
                $"The person's faces could not be loaded: {exception.Message}";
        }
    }

    private async void OnUnassignFaceClick(
        object sender,
        RoutedEventArgs eventArgs)
    {
        if (sender is not FrameworkElement { Tag: PersonFaceRow row }
            || row.Face.PersonId is not { } personId
            || PeopleList.SelectedItem is not PersonRow selected)
        {
            return;
        }

        try
        {
            await catalog.AssignFacesAsync([row.Face.Id], null);

            // When that was the person's last face on the photo, the keyword
            // written earlier no longer holds and comes back out; the region
            // list is rebuilt either way.
            var stillPresent = (await catalog.GetFacesForPathAsync(row.Face.Path))
                .Any(face => face.PersonId == personId);
            if (!stillPresent)
            {
                await RemovePersonKeywordAsync(
                    row.Face.Path,
                    selected.Person.Name);
            }

            await WriteFaceRegionsAsync(row.Face.Path);
            DetailText.Text =
                $"Removed a face from “{selected.Person.Name}”; it returned "
                + "to the unnamed pool.";
        }
        catch (Exception exception)
        {
            DetailText.Text = $"Removing the face failed: {exception.Message}";
        }

        await RefreshAsync();
    }

    /// <summary>
    /// Takes the person's name back out of the photograph's keywords - the
    /// mirror of <see cref="WritePersonKeywordAsync"/> for corrections.
    /// </summary>
    private async Task RemovePersonKeywordAsync(string path, string name)
    {
        var record = await catalog.GetByPathAsync(path);
        if (record is null)
        {
            return;
        }

        var remaining = record.KeywordList
            .Where(keyword => !keyword.Equals(
                name,
                StringComparison.OrdinalIgnoreCase))
            .ToArray();
        var joined = remaining.Length == 0
            ? null
            : PhotoRecord.JoinKeywords(remaining);
        if (!string.Equals(joined, record.Keywords, StringComparison.Ordinal))
        {
            await catalog.UpdateKeywordsAsync(path, joined);
        }
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
            // The face regions are rebuilt whole, so they carry the new name.
            var faces = await catalog.GetFacesForPersonAsync(row.Person.Id);
            foreach (var path in faces
                         .Select(face => face.Path)
                         .Distinct(StringComparer.OrdinalIgnoreCase))
            {
                await WritePersonKeywordAsync(path, name);
                await WriteFaceRegionsAsync(path);
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
        ScanButton.Content = scanning ? "Stop" : ScanButtonLabel;
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
