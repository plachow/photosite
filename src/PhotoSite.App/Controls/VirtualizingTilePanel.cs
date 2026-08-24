using System.Collections.Specialized;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Controls.Primitives;
using System.Windows.Media;

namespace PhotoSite.Controls;

public sealed class VirtualizingTilePanel : VirtualizingPanel, IScrollInfo
{
    public static readonly DependencyProperty ItemWidthProperty =
        DependencyProperty.Register(
            nameof(ItemWidth),
            typeof(double),
            typeof(VirtualizingTilePanel),
            new FrameworkPropertyMetadata(
                196d,
                FrameworkPropertyMetadataOptions.AffectsMeasure));

    public static readonly DependencyProperty ItemHeightProperty =
        DependencyProperty.Register(
            nameof(ItemHeight),
            typeof(double),
            typeof(VirtualizingTilePanel),
            new FrameworkPropertyMetadata(
                166d,
                FrameworkPropertyMetadataOptions.AffectsMeasure));

    private Size extent;
    private Size viewport;
    private Point offset;
    private int itemsPerRow = 1;

    public double ItemWidth
    {
        get => (double)GetValue(ItemWidthProperty);
        set => SetValue(ItemWidthProperty, value);
    }

    public double ItemHeight
    {
        get => (double)GetValue(ItemHeightProperty);
        set => SetValue(ItemHeightProperty, value);
    }

    protected override Size MeasureOverride(Size availableSize)
    {
        var itemCount = ItemsControl.GetItemsOwner(this)?.Items.Count ?? 0;
        var width = double.IsInfinity(availableSize.Width)
            ? Math.Max(ItemWidth, viewport.Width)
            : availableSize.Width;
        var height = double.IsInfinity(availableSize.Height)
            ? viewport.Height
            : availableSize.Height;

        itemsPerRow = Math.Max(1, (int)Math.Floor(width / ItemWidth));
        viewport = new Size(width, Math.Max(0, height));
        extent = new Size(
            width,
            Math.Ceiling(itemCount / (double)itemsPerRow) * ItemHeight);
        var maximumOffset = Math.Max(0, extent.Height - viewport.Height);
        if (offset.Y > maximumOffset)
        {
            offset.Y = maximumOffset;
        }

        ScrollOwner?.InvalidateScrollInfo();

        if (itemCount == 0)
        {
            RemoveInternalChildRange(0, InternalChildren.Count);
            return availableSize;
        }

        var firstRow = Math.Max(0, (int)Math.Floor(offset.Y / ItemHeight));
        var visibleRows = Math.Max(
            1,
            (int)Math.Ceiling(viewport.Height / ItemHeight) + 1);
        var firstIndex = Math.Min(itemCount - 1, firstRow * itemsPerRow);
        var lastIndex = Math.Min(
            itemCount - 1,
            ((firstRow + visibleRows) * itemsPerRow) - 1);

        CleanUpItems(firstIndex, lastIndex);
        RealizeItems(firstIndex, lastIndex);
        return availableSize;
    }

    /// <summary>
    /// ScrollIntoView lands here when the item is virtualized away and no
    /// container exists to bring into view; the base implementation is a
    /// silent no-op, which left the list sitting wherever it was.
    /// </summary>
    protected override void BringIndexIntoView(int index)
    {
        if (index < 0)
        {
            return;
        }

        var top = (index / itemsPerRow) * ItemHeight;
        if (top < VerticalOffset)
        {
            SetVerticalOffset(top);
        }
        else if (top + ItemHeight > VerticalOffset + ViewportHeight)
        {
            SetVerticalOffset(top + ItemHeight - ViewportHeight);
        }
    }

    protected override Size ArrangeOverride(Size finalSize)
    {
        foreach (UIElement child in InternalChildren)
        {
            var index = PublicGenerator.IndexFromContainer(child);
            if (index < 0)
            {
                continue;
            }

            var row = index / itemsPerRow;
            var column = index % itemsPerRow;
            child.Arrange(
                new Rect(
                    column * ItemWidth,
                    (row * ItemHeight) - offset.Y,
                    ItemWidth,
                    ItemHeight));
        }

        return finalSize;
    }

    /// <summary>
    /// Only Reset clears the children for us (through the base panel), so
    /// granular notifications have to detach the affected containers by hand;
    /// otherwise InternalChildren drifts out of step with the generator and
    /// tiles start rendering duplicated or in the wrong slot.
    /// </summary>
    protected override void OnItemsChanged(
        object sender,
        ItemsChangedEventArgs args)
    {
        switch (args.Action)
        {
            case NotifyCollectionChangedAction.Remove:
            case NotifyCollectionChangedAction.Replace:
                RemoveContainers(args.Position, args.ItemUICount);
                break;
            case NotifyCollectionChangedAction.Move:
                RemoveContainers(args.OldPosition, args.ItemUICount);
                break;
        }

        InvalidateMeasure();
    }

    private void RemoveContainers(GeneratorPosition position, int containerCount)
    {
        if (containerCount <= 0)
        {
            return;
        }

        var childIndex = position.Offset > 0 ? position.Index + 1 : position.Index;
        if (childIndex < 0 || childIndex >= InternalChildren.Count)
        {
            return;
        }

        RemoveInternalChildRange(
            childIndex,
            Math.Min(containerCount, InternalChildren.Count - childIndex));
    }

    private void RealizeItems(int firstIndex, int lastIndex)
    {
        var generator = Generator;
        var startPosition = generator.GeneratorPositionFromIndex(firstIndex);
        var childIndex = Math.Max(
            0,
            startPosition.Offset == 0
                ? startPosition.Index
                : startPosition.Index + 1);

        using (generator.StartAt(
                   startPosition,
                   GeneratorDirection.Forward,
                   true))
        {
            for (var itemIndex = firstIndex;
                 itemIndex <= lastIndex;
                 itemIndex++, childIndex++)
            {
                var child = (UIElement)generator.GenerateNext(out var newlyRealized);
                if (newlyRealized)
                {
                    InsertContainer(childIndex, child);
                    generator.PrepareItemContainer(child);
                }
                else if (childIndex >= InternalChildren.Count
                         || !ReferenceEquals(InternalChildren[childIndex], child))
                {
                    // A Move keeps the container realized but detaches it from
                    // InternalChildren, so it has to be slotted back by hand.
                    var attachedAt = InternalChildren.IndexOf(child);
                    if (attachedAt >= 0)
                    {
                        RemoveInternalChildRange(attachedAt, 1);
                    }

                    InsertContainer(childIndex, child);
                }

                child.Measure(new Size(ItemWidth, ItemHeight));
            }
        }
    }

    private void InsertContainer(int childIndex, UIElement child)
    {
        if (childIndex >= InternalChildren.Count)
        {
            AddInternalChild(child);
        }
        else
        {
            InsertInternalChild(childIndex, child);
        }
    }

    private void CleanUpItems(int firstIndex, int lastIndex)
    {
        var generator = Generator;
        for (var childIndex = InternalChildren.Count - 1; childIndex >= 0; childIndex--)
        {
            var position = new GeneratorPosition(childIndex, 0);
            var itemIndex = generator.IndexFromGeneratorPosition(position);
            if (itemIndex >= firstIndex && itemIndex <= lastIndex)
            {
                continue;
            }

            if (itemIndex < 0)
            {
                // The generator no longer knows this slot (it can lag behind
                // InternalChildren for one pass after a granular change);
                // dropping the child alone keeps the two back in step.
                RemoveInternalChildRange(childIndex, 1);
                continue;
            }

            generator.Remove(position, 1);
            RemoveInternalChildRange(childIndex, 1);
        }
    }

    public bool CanHorizontallyScroll { get; set; }

    public bool CanVerticallyScroll { get; set; } = true;

    public double ExtentHeight => extent.Height;

    public double ExtentWidth => extent.Width;

    public double HorizontalOffset => offset.X;

    public ScrollViewer? ScrollOwner { get; set; }

    public double VerticalOffset => offset.Y;

    public double ViewportHeight => viewport.Height;

    public double ViewportWidth => viewport.Width;

    public void LineDown() => SetVerticalOffset(VerticalOffset + 32);

    public void LineLeft()
    {
    }

    public void LineRight()
    {
    }

    public void LineUp() => SetVerticalOffset(VerticalOffset - 32);

    public Rect MakeVisible(Visual visual, Rect rectangle)
    {
        var child = visual as DependencyObject;
        while (child is not null && child is not ListBoxItem)
        {
            child = VisualTreeHelper.GetParent(child);
        }

        if (child is null)
        {
            return rectangle;
        }

        var index = PublicGenerator.IndexFromContainer(child);
        if (index < 0)
        {
            return rectangle;
        }

        var top = (index / itemsPerRow) * ItemHeight;
        if (top < VerticalOffset)
        {
            SetVerticalOffset(top);
        }
        else if (top + ItemHeight > VerticalOffset + ViewportHeight)
        {
            SetVerticalOffset(top + ItemHeight - ViewportHeight);
        }

        return new Rect(0, top, ItemWidth, ItemHeight);
    }

    public void MouseWheelDown() => SetVerticalOffset(VerticalOffset + 3 * 32);

    public void MouseWheelLeft()
    {
    }

    public void MouseWheelRight()
    {
    }

    public void MouseWheelUp() => SetVerticalOffset(VerticalOffset - 3 * 32);

    public void PageDown() => SetVerticalOffset(VerticalOffset + ViewportHeight);

    public void PageLeft()
    {
    }

    public void PageRight()
    {
    }

    public void PageUp() => SetVerticalOffset(VerticalOffset - ViewportHeight);

    public void SetHorizontalOffset(double newOffset)
    {
    }

    public void SetVerticalOffset(double newOffset)
    {
        var clamped = Math.Clamp(
            newOffset,
            0,
            Math.Max(0, ExtentHeight - ViewportHeight));
        if (Math.Abs(clamped - offset.Y) < 0.1)
        {
            return;
        }

        offset.Y = clamped;
        ScrollOwner?.InvalidateScrollInfo();
        InvalidateMeasure();
    }

    private IItemContainerGenerator Generator => ItemContainerGenerator;

    private ItemContainerGenerator PublicGenerator =>
        ItemsControl.GetItemsOwner(this)?.ItemContainerGenerator
        ?? throw new InvalidOperationException(
            "VirtualizingTilePanel must be hosted by an ItemsControl.");
}
