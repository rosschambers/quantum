import { describe, it, expect, vi, afterEach } from 'vitest';
import { render, fireEvent, cleanup } from '@testing-library/svelte/svelte5';
import ImageRenderer from './ImageRenderer.svelte';
import imageRendererSource from './ImageRenderer.svelte?raw';

const SAMPLE_URI =
    'data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=';

function stubNaturalSize(image: HTMLImageElement, naturalWidth: number, naturalHeight: number): void {
    Object.defineProperty(image, 'naturalWidth', { configurable: true, value: naturalWidth });
    Object.defineProperty(image, 'naturalHeight', { configurable: true, value: naturalHeight });
}

function stubPanCapability(viewport: HTMLElement): void {
    Object.defineProperty(viewport, 'setPointerCapture', { configurable: true, value: vi.fn() });
    Object.defineProperty(viewport, 'releasePointerCapture', { configurable: true, value: vi.fn() });
    Object.defineProperty(viewport, 'hasPointerCapture', { configurable: true, value: vi.fn().mockReturnValue(true) });
}

function stubOverflow(viewport: HTMLElement, clientSize: number, contentSize: number): void {
    Object.defineProperty(viewport, 'clientWidth', { configurable: true, value: clientSize });
    Object.defineProperty(viewport, 'clientHeight', { configurable: true, value: clientSize });
    Object.defineProperty(viewport, 'scrollWidth', { configurable: true, value: contentSize });
    Object.defineProperty(viewport, 'scrollHeight', { configurable: true, value: contentSize });
}

// jsdom has no layout engine. Supply measured layout, including padding,
// centered small images, scroll offsets, and native scroll-range clamping.
function stubImageGeometry(viewport: HTMLDivElement, image: HTMLImageElement): void {
    const viewportWidth = 900;
    const viewportHeight = 700;
    function dimensions(): { width: number; height: number } {
        const fittedScale = Math.min(1, (viewportWidth - 64) / image.naturalWidth, (viewportHeight - 64) / image.naturalHeight);
        return {
            width: image.style.width ? parseFloat(image.style.width) : image.naturalWidth * fittedScale,
            height: image.style.height ? parseFloat(image.style.height) : image.naturalHeight * fittedScale,
        };
    }
    Object.defineProperties(viewport, {
        clientWidth: { configurable: true, value: viewportWidth },
        clientHeight: { configurable: true, value: viewportHeight },
        scrollWidth: { configurable: true, get: () => Math.max(viewportWidth, dimensions().width + 64) },
        scrollHeight: { configurable: true, get: () => Math.max(viewportHeight, dimensions().height + 64) },
    });
    let scrollLeft = 0;
    let scrollTop = 0;
    Object.defineProperties(viewport, {
        scrollLeft: {
            configurable: true,
            get: () => scrollLeft,
            set: (value: number) => { scrollLeft = Math.max(0, Math.min(value, viewport.scrollWidth - viewportWidth)); },
        },
        scrollTop: {
            configurable: true,
            get: () => scrollTop,
            set: (value: number) => { scrollTop = Math.max(0, Math.min(value, viewport.scrollHeight - viewportHeight)); },
        },
    });
    viewport.getBoundingClientRect = () => new DOMRect(20, 78, viewportWidth, viewportHeight);
    image.getBoundingClientRect = () => {
        const { width, height } = dimensions();
        return new DOMRect(
            20 + Math.max(32, (viewportWidth - width) / 2) - viewport.scrollLeft,
            78 + Math.max(32, (viewportHeight - height) / 2) - viewport.scrollTop,
            width, height,
        );
    };
}

function imagePointAtCenter(viewport: HTMLDivElement, image: HTMLImageElement): { x: number; y: number } {
    const viewportBounds = viewport.getBoundingClientRect();
    const imageBounds = image.getBoundingClientRect();
    return {
        x: (viewportBounds.left + viewport.clientWidth / 2 - imageBounds.left) * image.naturalWidth / imageBounds.width,
        y: (viewportBounds.top + viewport.clientHeight / 2 - imageBounds.top) * image.naturalHeight / imageBounds.height,
    };
}

afterEach(() => {
    cleanup();
    vi.restoreAllMocks();
});

describe('ImageRenderer', () => {
    it('centers with nonnegative auto margins rather than unsafe overflow alignment', () => {
        const source = imageRendererSource;
        const viewportStyles = source.match(/\.viewport \{([^}]+)\}/)?.[1] ?? '';
        const imageStyles = source.match(/\n  img \{([^}]+)\}/)?.[1] ?? '';
        expect(viewportStyles).not.toMatch(/(?:align-items|justify-content): center/);
        expect(imageStyles).toMatch(/margin: auto/);
        expect(viewportStyles).toContain('overflow: auto');
    });

    it('keeps the measured image center through Fit, Actual size, and repeated zoom with padding', async () => {
        const { getByAltText, getByLabelText } = render(ImageRenderer, { props: { uri: SAMPLE_URI, filename: 'photo.png' } });
        const image = getByAltText('photo.png') as HTMLImageElement;
        const viewport = getByLabelText('Image pan area') as HTMLDivElement;
        stubNaturalSize(image, 1600, 1200);
        stubImageGeometry(viewport, image);
        await fireEvent.load(image);
        expect(imagePointAtCenter(viewport, image)).toEqual({ x: 800, y: 600 });
        await fireEvent.click(getByLabelText('Actual size'));
        expect(imagePointAtCenter(viewport, image)).toEqual({ x: 800, y: 600 });
        viewport.scrollLeft += 100;
        viewport.scrollTop += 80;
        const anchor = imagePointAtCenter(viewport, image);
        await fireEvent.click(getByLabelText('Zoom in'));
        expect(imagePointAtCenter(viewport, image)).toEqual(anchor);
        await fireEvent.click(getByLabelText('Zoom out'));
        expect(imagePointAtCenter(viewport, image)).toEqual(anchor);
    });

    it('keeps a centered small image centered when zoom cannot scroll', async () => {
        const { getByAltText, getByLabelText } = render(ImageRenderer, { props: { uri: SAMPLE_URI, filename: 'small.png' } });
        const image = getByAltText('small.png') as HTMLImageElement;
        const viewport = getByLabelText('Image pan area') as HTMLDivElement;
        stubNaturalSize(image, 600, 400);
        stubImageGeometry(viewport, image);
        await fireEvent.load(image);
        await fireEvent.click(getByLabelText('Actual size'));
        await fireEvent.click(getByLabelText('Zoom in'));
        await fireEvent.click(getByLabelText('Zoom in'));
        expect(imagePointAtCenter(viewport, image)).toEqual({ x: 300, y: 200 });
        await fireEvent.click(getByLabelText('Zoom out'));
        expect(viewport.scrollLeft).toBe(0);
        expect(viewport.scrollTop).toBe(0);
        expect(imagePointAtCenter(viewport, image)).toEqual({ x: 300, y: 200 });
    });

    it.each([
        { horizontalEnd: false, verticalEnd: false },
        { horizontalEnd: true, verticalEnd: false },
        { horizontalEnd: false, verticalEnd: true },
        { horizontalEnd: true, verticalEnd: true },
    ])('clamps to the nearest reachable center at corner $horizontalEnd/$verticalEnd', async ({ horizontalEnd, verticalEnd }) => {
        const { getByAltText, getByLabelText } = render(ImageRenderer, { props: { uri: SAMPLE_URI, filename: 'photo.png' } });
        const image = getByAltText('photo.png') as HTMLImageElement;
        const viewport = getByLabelText('Image pan area') as HTMLDivElement;
        stubNaturalSize(image, 1600, 1200);
        stubImageGeometry(viewport, image);
        await fireEvent.load(image);
        await fireEvent.click(getByLabelText('Actual size'));
        await fireEvent.click(getByLabelText('Zoom in'));
        viewport.scrollLeft = horizontalEnd ? viewport.scrollWidth - viewport.clientWidth : 0;
        viewport.scrollTop = verticalEnd ? viewport.scrollHeight - viewport.clientHeight : 0;
        await fireEvent.click(getByLabelText('Zoom out'));
        expect(viewport.scrollLeft).toBe(horizontalEnd ? 764 : 0);
        expect(viewport.scrollTop).toBe(verticalEnd ? 564 : 0);
        const nearestAnchor = { x: horizontalEnd ? 1182 : 418, y: verticalEnd ? 882 : 318 };
        expect(imagePointAtCenter(viewport, image)).toEqual(nearestAnchor);
        await fireEvent.click(getByLabelText('Zoom in'));
        expect(imagePointAtCenter(viewport, image)).toEqual(nearestAnchor);
    });

    it('does not begin a pan from a toolbar control', async () => {
        const { getByAltText, getByLabelText } = render(ImageRenderer, { props: { uri: SAMPLE_URI, filename: 'photo.png' } });
        const image = getByAltText('photo.png') as HTMLImageElement;
        const viewport = getByLabelText('Image pan area') as HTMLDivElement;
        stubNaturalSize(image, 2000, 1500);
        await fireEvent.load(image);
        await fireEvent.click(getByLabelText('Actual size'));
        stubOverflow(viewport, 300, 2000);
        stubPanCapability(viewport);
        await fireEvent.pointerDown(getByLabelText('Zoom in'), { pointerId: 7, clientX: 200, clientY: 200, button: 0 });
        expect(viewport.setPointerCapture).not.toHaveBeenCalled();
        expect(viewport.classList.contains('panning')).toBe(false);
    });

    it('stops after capture is already lost without trying to release it again', async () => {
        const { getByAltText, getByLabelText } = render(ImageRenderer, { props: { uri: SAMPLE_URI, filename: 'photo.png' } });
        const image = getByAltText('photo.png') as HTMLImageElement;
        const viewport = getByLabelText('Image pan area') as HTMLDivElement;
        stubNaturalSize(image, 2000, 1500);
        await fireEvent.load(image);
        await fireEvent.click(getByLabelText('Actual size'));
        stubOverflow(viewport, 300, 2000);
        stubPanCapability(viewport);
        viewport.scrollLeft = 100;
        await fireEvent.pointerDown(viewport, { pointerId: 7, clientX: 200, clientY: 200, button: 0 });
        await fireEvent(viewport, new PointerEvent('lostpointercapture', { pointerId: 8 }));
        expect(viewport.classList.contains('panning')).toBe(true);
        vi.mocked(viewport.hasPointerCapture).mockReturnValue(false);
        await fireEvent(viewport, new PointerEvent('lostpointercapture', { pointerId: 7 }));
        await fireEvent.pointerMove(viewport, { pointerId: 7, clientX: 100, clientY: 100 });
        expect(viewport.scrollLeft).toBe(100);
        expect(viewport.classList.contains('panning')).toBe(false);
        expect(viewport.releasePointerCapture).not.toHaveBeenCalled();
    });

    it.each(['resource change', 'Fit', 'capture loss', 'unmount'])('stops an active drag on %s', async (reason) => {
        const { getByAltText, getByLabelText, rerender, unmount } = render(ImageRenderer, { props: { uri: SAMPLE_URI, filename: 'photo.png' } });
        const image = getByAltText('photo.png') as HTMLImageElement;
        const viewport = getByLabelText('Image pan area') as HTMLDivElement;
        stubNaturalSize(image, 2000, 1500);
        await fireEvent.load(image);
        await fireEvent.click(getByLabelText('Actual size'));
        stubOverflow(viewport, 300, 2000);
        stubPanCapability(viewport);
        viewport.scrollLeft = 100;
        viewport.scrollTop = 80;
        await fireEvent.pointerDown(viewport, { pointerId: 7, clientX: 200, clientY: 200, button: 0 });
        expect(viewport.classList.contains('panning')).toBe(true);
        if (reason === 'resource change') {
            await rerender({ uri: 'data:image/png;base64,new', filename: 'new.png' });
        } else if (reason === 'Fit') {
            await fireEvent.click(getByLabelText('Fit image to window'));
        } else if (reason === 'capture loss') {
            await fireEvent(viewport, new PointerEvent('lostpointercapture', { pointerId: 7 }));
        } else {
            await unmount();
        }
        expect(viewport.releasePointerCapture).toHaveBeenCalledWith(7);
        if (reason !== 'unmount') expect(viewport.classList.contains('panning')).toBe(false);
        if (reason === 'resource change') {
            expect(viewport.scrollLeft).toBe(0);
            expect(viewport.scrollTop).toBe(0);
            const nextImage = getByAltText('new.png') as HTMLImageElement;
            stubNaturalSize(nextImage, 2000, 1500);
            await fireEvent.load(nextImage);
            await fireEvent.click(getByLabelText('Actual size'));
            viewport.scrollLeft = 100;
            viewport.scrollTop = 80;
        }
        const scrollLeft = viewport.scrollLeft;
        const scrollTop = viewport.scrollTop;
        await fireEvent.pointerMove(viewport, { pointerId: 7, clientX: 100, clientY: 100 });
        expect(viewport.scrollLeft).toBe(scrollLeft);
        expect(viewport.scrollTop).toBe(scrollTop);
    });

    it('defaults to Fit mode', () => {
        const { container } = render(ImageRenderer, { props: { uri: SAMPLE_URI, filename: 'x.png' } });
        expect(container.querySelector('.mode-fit')?.classList.contains('active')).toBe(true);
    });

    it('a load error renders a visible message, not a blank pane', async () => {
        const { container, getByAltText } = render(ImageRenderer, {
            props: { uri: 'data:image/png;base64,invalid', filename: 'bad.png' },
        });
        await fireEvent.error(getByAltText('bad.png'));
        expect(container.querySelector('.image-error')).not.toBeNull();
        expect(container.textContent).toContain('bad.png');
    });

    it('hides the zoom toolbar once an error is showing, instead of a dead pane', async () => {
        const { container, getByAltText } = render(ImageRenderer, {
            props: { uri: 'data:image/png;base64,invalid', filename: 'bad.png' },
        });
        await fireEvent.error(getByAltText('bad.png'));
        expect(container.querySelector('.toolbar')).toBeNull();
    });

    it('does not stretch a small natural image while fitted', async () => {
        const { getByAltText } = render(ImageRenderer, { props: { uri: SAMPLE_URI, filename: 'small.png' } });
        const image = getByAltText('small.png') as HTMLImageElement;
        stubNaturalSize(image, 1, 1);
        await fireEvent.load(image);
        expect(image.style.width).toBe('');
        expect(image.style.height).toBe('');
    });

    it('switches to Actual size at literal one to one natural pixel dimensions', async () => {
        const { container, getByAltText, getByLabelText } = render(ImageRenderer, {
            props: { uri: SAMPLE_URI, filename: 'photo.png' },
        });
        const image = getByAltText('photo.png') as HTMLImageElement;
        stubNaturalSize(image, 400, 300);
        await fireEvent.load(image);

        await fireEvent.click(getByLabelText('Actual size'));

        expect(image.style.width).toBe('400px');
        expect(image.style.height).toBe('300px');
        expect(container.querySelector('.zoom-label')?.textContent).toBe('100%');
        expect(container.querySelector('.mode-actual')?.classList.contains('active')).toBe(true);
    });

    it('zoom in from Fit lands on actual size first, then steps up by a bounded factor', async () => {
        const { container, getByAltText, getByLabelText } = render(ImageRenderer, {
            props: { uri: SAMPLE_URI, filename: 'photo.png' },
        });
        const image = getByAltText('photo.png') as HTMLImageElement;
        stubNaturalSize(image, 200, 100);
        await fireEvent.load(image);

        await fireEvent.click(getByLabelText('Zoom in'));
        expect(image.style.width).toBe('200px');
        expect(container.querySelector('.zoom-label')?.textContent).toBe('100%');

        await fireEvent.click(getByLabelText('Zoom in'));
        expect(image.style.width).toBe('250px');
        expect(container.querySelector('.zoom-label')?.textContent).toBe('125%');
    });

    it('zoom out steps back down by the same bounded factor', async () => {
        const { container, getByAltText, getByLabelText } = render(ImageRenderer, {
            props: { uri: SAMPLE_URI, filename: 'photo.png' },
        });
        const image = getByAltText('photo.png') as HTMLImageElement;
        stubNaturalSize(image, 200, 100);
        await fireEvent.load(image);

        await fireEvent.click(getByLabelText('Actual size'));
        await fireEvent.click(getByLabelText('Zoom in'));
        expect(container.querySelector('.zoom-label')?.textContent).toBe('125%');

        await fireEvent.click(getByLabelText('Zoom out'));
        expect(container.querySelector('.zoom-label')?.textContent).toBe('100%');
    });

    it('never distorts aspect ratio at any zoom level', async () => {
        const { getByAltText, getByLabelText } = render(ImageRenderer, {
            props: { uri: SAMPLE_URI, filename: 'photo.png' },
        });
        const image = getByAltText('photo.png') as HTMLImageElement;
        stubNaturalSize(image, 300, 150);
        await fireEvent.load(image);

        await fireEvent.click(getByLabelText('Zoom in'));
        await fireEvent.click(getByLabelText('Zoom in'));
        await fireEvent.click(getByLabelText('Zoom in'));

        const width = parseFloat(image.style.width);
        const height = parseFloat(image.style.height);
        expect(width / height).toBeCloseTo(300 / 150, 5);
    });

    it('keeps zoom within finite, safe bounds no matter how many times it is pressed', async () => {
        const { getByAltText, getByLabelText } = render(ImageRenderer, {
            props: { uri: SAMPLE_URI, filename: 'photo.png' },
        });
        const image = getByAltText('photo.png') as HTMLImageElement;
        stubNaturalSize(image, 100, 100);
        await fireEvent.load(image);

        for (let step = 0; step < 40; step += 1) {
            await fireEvent.click(getByLabelText('Zoom in'));
        }

        const width = parseFloat(image.style.width);
        expect(Number.isFinite(width)).toBe(true);
        expect(width).toBeLessThanOrEqual(100 * 8);
    });

    it('the zoom out control is disabled while fitted, since fit is already the smallest sensible view', () => {
        const { getByLabelText } = render(ImageRenderer, { props: { uri: SAMPLE_URI, filename: 'photo.png' } });
        expect((getByLabelText('Zoom out') as HTMLButtonElement).disabled).toBe(true);
    });

    it('resets zoom, pan, and error state when the uri changes', async () => {
        const { container, getByAltText, getByLabelText, rerender } = render(ImageRenderer, {
            props: { uri: SAMPLE_URI, filename: 'a.png' },
        });
        let image = getByAltText('a.png') as HTMLImageElement;
        stubNaturalSize(image, 100, 100);
        await fireEvent.load(image);
        await fireEvent.click(getByLabelText('Actual size'));
        expect(container.querySelector('.mode-actual')?.classList.contains('active')).toBe(true);

        await rerender({ uri: 'data:image/png;base64,other', filename: 'b.png' });

        expect(container.querySelector('.mode-fit')?.classList.contains('active')).toBe(true);
        expect(container.querySelector('.zoom-label')?.textContent).toBe('Fit');
    });

    it('resets a prior error back to a loading image when the uri changes', async () => {
        const { container, getByAltText, rerender } = render(ImageRenderer, {
            props: { uri: 'data:image/png;base64,invalid', filename: 'bad.png' },
        });
        await fireEvent.error(getByAltText('bad.png'));
        expect(container.querySelector('.image-error')).not.toBeNull();

        await rerender({ uri: SAMPLE_URI, filename: 'good.png' });

        expect(container.querySelector('.image-error')).toBeNull();
        expect(getByAltText('good.png')).not.toBeNull();
    });

    it('drag-to-scroll pans the viewport when the image overflows at the current zoom', async () => {
        const { container, getByAltText, getByLabelText } = render(ImageRenderer, {
            props: { uri: SAMPLE_URI, filename: 'photo.png' },
        });
        const image = getByAltText('photo.png') as HTMLImageElement;
        stubNaturalSize(image, 2000, 2000);
        await fireEvent.load(image);
        await fireEvent.click(getByLabelText('Actual size'));

        const viewport = container.querySelector('.viewport') as HTMLDivElement;
        stubPanCapability(viewport);
        stubOverflow(viewport, 300, 2000);
        viewport.scrollLeft = 100;
        viewport.scrollTop = 100;

        await fireEvent.pointerDown(viewport, { pointerId: 1, clientX: 200, clientY: 200, button: 0 });
        await fireEvent.pointerMove(viewport, { pointerId: 1, clientX: 150, clientY: 170 });

        expect(viewport.scrollLeft).toBe(150);
        expect(viewport.scrollTop).toBe(130);

        await fireEvent.pointerUp(viewport, { pointerId: 1, clientX: 150, clientY: 170 });
        expect(viewport.releasePointerCapture).toHaveBeenCalledWith(1);
    });

    it('does not pan while fitted, since there is nothing to scroll', async () => {
        const { container, getByAltText } = render(ImageRenderer, { props: { uri: SAMPLE_URI, filename: 'photo.png' } });
        const image = getByAltText('photo.png') as HTMLImageElement;
        stubNaturalSize(image, 100, 100);
        await fireEvent.load(image);

        const viewport = container.querySelector('.viewport') as HTMLDivElement;
        stubPanCapability(viewport);
        stubOverflow(viewport, 300, 100);
        viewport.scrollLeft = 0;

        await fireEvent.pointerDown(viewport, { pointerId: 1, clientX: 200, clientY: 200, button: 0 });
        await fireEvent.pointerMove(viewport, { pointerId: 1, clientX: 100, clientY: 100 });

        expect(viewport.scrollLeft).toBe(0);
        expect(viewport.setPointerCapture).not.toHaveBeenCalled();
    });

    it('exposes an accessible role and label for the pan area', () => {
        const { getByRole } = render(ImageRenderer, { props: { uri: SAMPLE_URI, filename: 'photo.png' } });
        expect(getByRole('group', { name: 'Image pan area' })).not.toBeNull();
    });

    it('local plus and minus keys zoom the image only when the pan area itself is focused', async () => {
        const windowKeydown = vi.fn();
        window.addEventListener('keydown', windowKeydown);
        const { container, getByRole } = render(ImageRenderer, { props: { uri: SAMPLE_URI, filename: 'photo.png' } });
        const pan = getByRole('group', { name: 'Image pan area' });

        await fireEvent.keyDown(pan, { key: '+' });

        expect(container.querySelector('.zoom-label')?.textContent).toBe('100%');
        expect(windowKeydown).not.toHaveBeenCalled();

        window.removeEventListener('keydown', windowKeydown);
    });

    it('a wheel over the pan area is left as native scroll, never hijacked', async () => {
        const { container } = render(ImageRenderer, { props: { uri: SAMPLE_URI, filename: 'photo.png' } });
        const viewport = container.querySelector('.viewport') as HTMLDivElement;
        const event = await fireEvent.wheel(viewport, { deltaY: 10 });
        expect(event).toBe(true); // fireEvent returns false only when preventDefault was called
    });

    it('does not render a draggable native ghost image', () => {
        const { getByAltText } = render(ImageRenderer, { props: { uri: SAMPLE_URI, filename: 'photo.png' } });
        const image = getByAltText('photo.png') as HTMLImageElement;
        expect(image.draggable).toBe(false);
    });
});
