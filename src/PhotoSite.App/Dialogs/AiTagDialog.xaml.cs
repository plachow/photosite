using System.Net.Http;
using System.Windows;
using System.Windows.Media.Imaging;
using PhotoSite.Infrastructure;
using PhotoSite.Services;
using PhotoSite.ViewModels;

namespace PhotoSite.Dialogs;

/// <summary>
/// Runs a local Ollama vision model over the selected photographs and writes
/// the returned title, description and keywords through the same catalogue
/// and outbox path as a manual edit, so they end up in the files too.
/// </summary>
public partial class AiTagDialog : Window
{
    private const string EndpointSetting = "ai_endpoint";
    private const string ModelSetting = "ai_model";
    private const string LanguageSetting = "ai_language";
    private const string ApplyModeSetting = "ai_apply_mode";

    /// <summary>
    /// The longest side sent to the model. A vision model reads a photograph
    /// comfortably at this size, and it keeps the request far smaller than
    /// the original file.
    /// </summary>
    private const int RequestPixelWidth = 1024;

    private readonly IReadOnlyList<PhotoItemViewModel> photos;
    private readonly OllamaVisionService service;
    private readonly PhotoCatalogRepository catalog;
    private readonly PreviewService previews;
    private readonly ExifToolGeolocator geolocator;
    private CancellationTokenSource? runCancellation;
    private bool isLoading;
    private bool isRunning;

    internal AiTagDialog(
        IReadOnlyList<PhotoItemViewModel> photos,
        OllamaVisionService service,
        PhotoCatalogRepository catalog,
        PreviewService previews,
        ExifToolGeolocator geolocator)
    {
        this.photos = photos;
        this.service = service;
        this.catalog = catalog;
        this.previews = previews;
        this.geolocator = geolocator;
        InitializeComponent();
        DarkWindowChrome.Apply(this);
        PopulateChoices();
        Loaded += async (_, _) => await LoadSettingsAsync();
    }

    /// <summary>How many photographs a run has described so far.</summary>
    public int DescribedCount { get; private set; }

    private void PopulateChoices()
    {
        isLoading = true;
        LanguageBox.ItemsSource = new[] { "Czech", "English" };
        LanguageBox.SelectedIndex = 0;
        ApplyModeBox.ItemsSource = new[]
        {
            new Choice(AiApplyMode.FillEmpty, "Fill only empty fields"),
            new Choice(AiApplyMode.Overwrite, "Overwrite with AI results")
        };
        ApplyModeBox.SelectedIndex = 0;
        EndpointBox.Text = OllamaVisionService.DefaultEndpoint;
        ModelBox.Text = OllamaVisionService.DefaultModel;
        isLoading = false;
    }

    private async Task LoadSettingsAsync()
    {
        isLoading = true;
        try
        {
            if (await catalog.GetSettingAsync(EndpointSetting)
                is { Length: > 0 } endpoint)
            {
                EndpointBox.Text = endpoint;
            }

            if (await catalog.GetSettingAsync(ModelSetting)
                is { Length: > 0 } model)
            {
                ModelBox.Text = model;
            }

            if (await catalog.GetSettingAsync(LanguageSetting)
                is { Length: > 0 } language
                && LanguageBox.Items.Contains(language))
            {
                LanguageBox.SelectedItem = language;
            }

            if (Enum.TryParse<AiApplyMode>(
                    await catalog.GetSettingAsync(ApplyModeSetting),
                    out var mode))
            {
                SelectApplyMode(mode);
            }
        }
        catch (Exception exception)
        {
            DetailText.Text =
                $"Saved settings could not be loaded: {exception.Message}";
        }
        finally
        {
            isLoading = false;
        }

        UpdateSummary();
        await RefreshModelsAsync();
    }

    private async void OnRefreshModelsClick(
        object sender,
        RoutedEventArgs eventArgs) =>
        await RefreshModelsAsync();

    private async Task RefreshModelsAsync()
    {
        try
        {
            var models = await service.ListModelsAsync(
                EndpointBox.Text,
                CancellationToken.None);
            var current = ModelBox.Text;
            isLoading = true;
            ModelBox.ItemsSource = models;
            ModelBox.Text = string.IsNullOrWhiteSpace(current)
                ? models.FirstOrDefault() ?? string.Empty
                : current;
            isLoading = false;
            if (models.Count == 0)
            {
                DetailText.Text = "The server offers no models.";
            }
        }
        catch (Exception exception)
            when (exception is HttpRequestException
                or OllamaVisionException
                or OperationCanceledException)
        {
            DetailText.Text =
                $"The model list could not be loaded: {Simplify(exception)}";
        }

        UpdateSummary();
    }

    private void OnSettingChanged(object sender, RoutedEventArgs eventArgs)
    {
        if (!isLoading)
        {
            UpdateSummary();
        }
    }

    private void UpdateSummary()
    {
        if (SummaryText is null || isRunning)
        {
            return;
        }

        var skipped = CountSkipped();
        var pending = photos.Count - skipped;
        var parts = new List<string>
        {
            pending == 1
                ? "1 photo will be described"
                : $"{pending:N0} of {photos.Count:N0} photos will be described"
        };
        if (skipped > 0)
        {
            parts.Add($"{skipped:N0} already described will be skipped");
        }

        SummaryText.Text = string.Join(" · ", parts);
        RunButton.IsEnabled = pending > 0
                              && !string.IsNullOrWhiteSpace(ModelBox.Text);
    }

    private int CountSkipped()
    {
        var mode = GetApplyMode();
        return photos.Count(photo => OllamaVisionService.ShouldSkip(
            mode,
            photo.Title,
            photo.Description));
    }

    private AiApplyMode GetApplyMode() =>
        ApplyModeBox.SelectedItem is Choice choice
            ? choice.Value
            : AiApplyMode.FillEmpty;

    private void SelectApplyMode(AiApplyMode mode)
    {
        foreach (var item in ApplyModeBox.ItemsSource.OfType<Choice>())
        {
            if (item.Value == mode)
            {
                ApplyModeBox.SelectedItem = item;
                return;
            }
        }
    }

    private async void OnRunClick(object sender, RoutedEventArgs eventArgs)
    {
        if (isRunning)
        {
            runCancellation?.Cancel();
            return;
        }

        var endpoint = EndpointBox.Text.Trim();
        var model = ModelBox.Text.Trim();
        var language = LanguageBox.SelectedItem as string ?? "Czech";
        var mode = GetApplyMode();
        await PersistSettingsAsync(endpoint, model, language, mode);

        // The model answers in the selected language; when that is not
        // English, the same call also returns an English description for the
        // catalogue, so both languages are searchable.
        var includeEnglish = !string.Equals(
            language,
            "English",
            StringComparison.OrdinalIgnoreCase);
        var pendingAtStart = photos.Count - CountSkipped();
        runCancellation = new CancellationTokenSource();
        var token = runCancellation.Token;
        SetRunningState(true);
        var described = 0;
        var skipped = 0;
        var failures = new List<string>();
        var stopMessage = (string?)null;
        var stopwatch = System.Diagnostics.Stopwatch.StartNew();

        try
        {
            for (var index = 0; index < photos.Count; index++)
            {
                token.ThrowIfCancellationRequested();
                var photo = photos[index];
                RunProgress.Value = 100d * index / photos.Count;
                SummaryText.Text =
                    $"{index + 1:N0} / {photos.Count:N0} · {photo.FileName}"
                    + DescribePace(
                        stopwatch.Elapsed,
                        described + failures.Count,
                        pendingAtStart);

                if (OllamaVisionService.ShouldSkip(
                        mode,
                        photo.Title,
                        photo.Description))
                {
                    skipped++;
                    continue;
                }

                if (!File.Exists(photo.Path))
                {
                    failures.Add($"{photo.FileName}: the file no longer exists");
                    continue;
                }

                try
                {
                    var jpeg = await LoadRequestImageAsync(photo, token);
                    // The place resolves offline from the coordinates the
                    // catalogue holds - which may be the user's correction,
                    // not what the file says. A failed lookup just means the
                    // model gets no place context.
                    var place = photo.Record is
                        { Latitude: { } latitude, Longitude: { } longitude }
                        ? await geolocator.ResolveAsync(latitude, longitude, token)
                        : null;
                    var insights = await service.DescribeAsync(
                        endpoint,
                        model,
                        jpeg,
                        language,
                        includeEnglish,
                        place,
                        photo.Record.HasApproximateLocation,
                        token);
                    Apply(photo, insights, mode, includeEnglish);
                    described++;
                    DescribedCount++;
                    DetailText.Text = DescribeResult(photo, insights);
                }
                catch (OperationCanceledException)
                    when (token.IsCancellationRequested)
                {
                    throw;
                }
                catch (HttpRequestException exception)
                {
                    // Every remaining photo would wait out the same connection
                    // failure, so an unreachable server ends the run.
                    stopMessage =
                        $"Ollama is not reachable at {endpoint}: {Simplify(exception)}";
                    break;
                }
                catch (Exception exception)
                {
                    // Whatever went wrong with this photograph - an undecodable
                    // file, a timed-out request, a malformed answer - the rest
                    // of the run must go on.
                    failures.Add($"{photo.FileName}: {Simplify(exception)}");
                    if (described == 0 && failures.Count == 3)
                    {
                        // Three failures before a single success point at the
                        // configuration, not at the photographs.
                        stopMessage = "The first photos all failed; check the "
                                      + "server address and the model name.";
                        break;
                    }
                }
            }
        }
        catch (OperationCanceledException)
        {
            stopMessage = "Stopped.";
        }
        finally
        {
            runCancellation?.Dispose();
            runCancellation = null;
            SetRunningState(false);
        }

        stopwatch.Stop();
        ReportOutcome(
            described,
            skipped,
            failures,
            stopMessage,
            stopwatch.Elapsed,
            DateTime.Now);
    }

    /// <summary>
    /// The running average and the projected finish, shown next to the
    /// progress counter once at least one photograph has been attempted.
    /// </summary>
    internal static string DescribePace(
        TimeSpan elapsed,
        int attempted,
        int pendingAtStart)
    {
        if (attempted == 0)
        {
            return string.Empty;
        }

        var average = elapsed / attempted;
        var remaining = Math.Max(0, pendingAtStart - attempted);
        var finish = DateTime.Now + average * remaining;
        return $" · {FormatDuration(average)}/photo"
               + $" · finish ~{finish:HH:mm}";
    }

    internal static string FormatDuration(TimeSpan duration) =>
        duration.TotalSeconds < 60
            ? $"{Math.Max(1, duration.TotalSeconds):0} s"
            : duration.TotalHours < 1
                ? $"{(int)duration.TotalMinutes} min {duration.Seconds:00} s"
                : $"{(int)duration.TotalHours} h {duration.Minutes:00} min";

    private async Task<byte[]> LoadRequestImageAsync(
        PhotoItemViewModel photo,
        CancellationToken cancellationToken)
    {
        // Never decode a photograph beyond its real size; a small image is
        // sent as-is rather than upscaled.
        var decodeWidth = photo.Record.PixelWidth is { } width and > 0
            ? Math.Min(RequestPixelWidth, width)
            : RequestPixelWidth;
        var image = await previews.LoadAsync(
            photo.Path,
            decodeWidth,
            cancellationToken);
        return await Task.Run(() => EncodeJpeg(image), cancellationToken);
    }

    private static byte[] EncodeJpeg(BitmapSource image)
    {
        var encoder = new JpegBitmapEncoder { QualityLevel = 85 };
        encoder.Frames.Add(BitmapFrame.Create(image));
        using var stream = new MemoryStream();
        encoder.Save(stream);
        return stream.ToArray();
    }

    private static void Apply(
        PhotoItemViewModel photo,
        AiPhotoInsights insights,
        AiApplyMode mode,
        bool primaryLanguageIsNotEnglish)
    {
        if (!string.IsNullOrWhiteSpace(insights.Title)
            && (mode == AiApplyMode.Overwrite
                || string.IsNullOrWhiteSpace(photo.Title)))
        {
            photo.Title = insights.Title;
        }

        if (!string.IsNullOrWhiteSpace(insights.Description)
            && (mode == AiApplyMode.Overwrite
                || string.IsNullOrWhiteSpace(photo.Description)))
        {
            photo.Description = insights.Description;
        }

        // When the run already speaks English, the primary description doubles
        // as the catalogue's English one.
        var english = primaryLanguageIsNotEnglish
            ? insights.DescriptionEn
            : insights.Description;
        if (!string.IsNullOrWhiteSpace(english)
            && (mode == AiApplyMode.Overwrite
                || string.IsNullOrWhiteSpace(photo.DescriptionEn)))
        {
            photo.DescriptionEn = english;
        }

        if (OllamaVisionService.MergeKeywords(photo.Keywords, insights.Keywords)
            is { } merged)
        {
            photo.Keywords = merged;
        }
    }

    private static string DescribeResult(
        PhotoItemViewModel photo,
        AiPhotoInsights insights)
    {
        var keywords = string.Join(", ", insights.Keywords.Take(8));
        return string.IsNullOrWhiteSpace(keywords)
            ? $"{photo.FileName}: {insights.Title}"
            : $"{photo.FileName}: {insights.Title} · {keywords}";
    }

    private void ReportOutcome(
        int described,
        int skipped,
        IReadOnlyList<string> failures,
        string? stopMessage,
        TimeSpan elapsed,
        DateTime finishedAt)
    {
        var parts = new List<string>
        {
            $"Described {described:N0} photo(s)"
        };
        if (skipped > 0)
        {
            parts.Add($"{skipped:N0} skipped");
        }

        if (failures.Count > 0)
        {
            parts.Add($"{failures.Count:N0} failed");
        }

        if (stopMessage is not null)
        {
            parts.Add(stopMessage);
        }

        SummaryText.Text = string.Join(" · ", parts);

        // The last photo's metadata has served its purpose; the lasting
        // answer is how the whole run went.
        var attempted = described + failures.Count;
        var lines = new List<string>();
        if (attempted > 0)
        {
            lines.Add(
                $"{attempted:N0} photo(s) in {FormatDuration(elapsed)}"
                + $" ({FormatDuration(elapsed / attempted)} per photo)"
                + $" · finished at {finishedAt:HH:mm}");
        }

        lines.AddRange(failures.Take(6));
        if (failures.Count > 6)
        {
            lines.Add($"…and {failures.Count - 6:N0} more");
        }

        if (lines.Count > 0)
        {
            DetailText.Text = string.Join(Environment.NewLine, lines);
        }
    }

    private async Task PersistSettingsAsync(
        string endpoint,
        string model,
        string language,
        AiApplyMode mode)
    {
        try
        {
            await catalog.SetSettingAsync(EndpointSetting, endpoint);
            await catalog.SetSettingAsync(ModelSetting, model);
            await catalog.SetSettingAsync(LanguageSetting, language);
            await catalog.SetSettingAsync(ApplyModeSetting, mode.ToString());
        }
        catch (Exception exception)
        {
            DetailText.Text =
                $"The settings could not be saved: {exception.Message}";
        }
    }

    private void SetRunningState(bool running)
    {
        isRunning = running;
        RunProgress.Visibility = running
            ? Visibility.Visible
            : Visibility.Collapsed;
        RunProgress.Value = 0;
        RunButton.Content = running ? "Stop" : "Describe";
        RunButton.IsEnabled = true;
        CloseButton.IsEnabled = !running;
        EndpointBox.IsEnabled = !running;
        ModelBox.IsEnabled = !running;
        LanguageBox.IsEnabled = !running;
        ApplyModeBox.IsEnabled = !running;
        if (!running)
        {
            UpdateSummary();
        }
    }

    private static string Simplify(Exception exception) =>
        exception is OperationCanceledException
            ? "the request timed out"
            : exception.InnerException?.Message ?? exception.Message;

    private void OnCloseClick(object sender, RoutedEventArgs eventArgs)
    {
        DialogResult = DescribedCount > 0;
    }

    protected override void OnClosing(System.ComponentModel.CancelEventArgs e)
    {
        if (isRunning)
        {
            // The window stays open until the photograph in flight has been
            // finished or abandoned, so a result is never half-applied.
            e.Cancel = true;
            runCancellation?.Cancel();
            return;
        }

        base.OnClosing(e);
    }

    private sealed record Choice(AiApplyMode Value, string Label)
    {
        public override string ToString() => Label;
    }
}
