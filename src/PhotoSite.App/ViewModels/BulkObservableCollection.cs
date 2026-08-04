using System.Collections.ObjectModel;
using System.Collections.Specialized;
using System.ComponentModel;

namespace PhotoSite.ViewModels;

public sealed class BulkObservableCollection<T> : ObservableCollection<T>
{
    // Past this many granular notifications the container generator does more
    // work than a single Reset, so a wholesale reorder keeps resetting.
    private const int MaxIncrementalOperations = 128;

    /// <summary>
    /// Brings the collection to <paramref name="target"/> through granular
    /// Remove/Move/Insert notifications so realized containers - and the
    /// thumbnails they already decoded - survive the update. Raises nothing
    /// at all when the sequence is already the one the caller wants.
    /// </summary>
    public void SynchronizeTo(IReadOnlyList<T> target)
    {
        if (MatchesSequence(target))
        {
            return;
        }

        if (!TryApplyIncremental(target))
        {
            ReplaceRange(target);
        }
    }

    public void ReplaceRange(IEnumerable<T> items)
    {
        Items.Clear();
        foreach (var item in items)
        {
            Items.Add(item);
        }

        OnPropertyChanged(new PropertyChangedEventArgs(nameof(Count)));
        OnPropertyChanged(new PropertyChangedEventArgs("Item[]"));
        OnCollectionChanged(
            new NotifyCollectionChangedEventArgs(NotifyCollectionChangedAction.Reset));
    }

    private bool MatchesSequence(IReadOnlyList<T> target)
    {
        if (Items.Count != target.Count)
        {
            return false;
        }

        for (var index = 0; index < target.Count; index++)
        {
            if (!EqualityComparer<T>.Default.Equals(Items[index], target[index]))
            {
                return false;
            }
        }

        return true;
    }

    private bool TryApplyIncremental(IReadOnlyList<T> target)
    {
        var desired = new HashSet<T>(target);
        var current = new HashSet<T>(Items);
        var survivors = Items.Where(desired.Contains).ToArray();
        var expected = target.Where(current.Contains).ToArray();

        var operations = (Items.Count - survivors.Length)
                         + (target.Count - survivors.Length);
        for (var index = 0; index < survivors.Length; index++)
        {
            if (!EqualityComparer<T>.Default.Equals(survivors[index], expected[index]))
            {
                operations++;
            }
        }

        if (operations > MaxIncrementalOperations)
        {
            return false;
        }

        for (var index = Items.Count - 1; index >= 0; index--)
        {
            if (!desired.Contains(Items[index]))
            {
                RemoveAt(index);
            }
        }

        for (var index = 0; index < target.Count; index++)
        {
            var wanted = target[index];
            if (index < Count
                && EqualityComparer<T>.Default.Equals(this[index], wanted))
            {
                continue;
            }

            var existing = IndexOf(wanted);
            if (existing >= 0)
            {
                Move(existing, index);
            }
            else
            {
                Insert(index, wanted);
            }
        }

        return true;
    }
}
