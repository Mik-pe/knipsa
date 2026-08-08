import java.io.BufferedInputStream;
import java.util.ArrayList;
import java.util.List;
import java.util.Locale;
import java.util.Scanner;
import org.locationtech.jts.geom.Coordinate;
import org.locationtech.jts.geom.Geometry;
import org.locationtech.jts.geom.GeometryCollection;
import org.locationtech.jts.geom.GeometryFactory;
import org.locationtech.jts.geom.Polygon;
import org.locationtech.jts.geom.util.GeometryFixer;
import org.locationtech.jts.operation.overlayng.OverlayNG;
import org.locationtech.jts.operation.overlayng.OverlayNGRobust;

/** Calibrated JTS reference adapter for Knipsa's whitespace workload protocol. */
public final class KnipsaJtsAdapter {
    private static final int WARMUP_RUNS = 3;
    private static final int SAMPLE_RUNS = 25;
    private static final long MINIMUM_SAMPLE_TIME_NS = 2_000_000L;
    private static final int MAXIMUM_ITERATIONS_PER_SAMPLE = 1 << 20;
    private static final GeometryFactory FACTORY = new GeometryFactory();

    private KnipsaJtsAdapter() {}

    public static void main(String[] arguments) {
        Locale.setDefault(Locale.ROOT);
        System.out.printf(
                Locale.ROOT,
                "{\"implementation\":\"jts-overlayng-robust\",\"revision\":\"jts-core-1.20.0\",\"samples\":%d,\"warmups\":%d,\"minimum_sample_time_ns\":%d}%n",
                SAMPLE_RUNS,
                WARMUP_RUNS,
                MINIMUM_SAMPLE_TIME_NS);

        try (Scanner input = new Scanner(new BufferedInputStream(System.in))) {
            input.useLocale(Locale.ROOT);
            while (input.hasNext()) {
                String id = input.next();
                int operation = input.nextInt();
                int fillRule = input.nextInt();
                try {
                    List<List<Coordinate>> subjectPaths = readPaths(input, input.nextInt());
                    List<List<Coordinate>> clipPaths = readPaths(input, input.nextInt());
                    if (fillRule != 0) {
                        throw new IllegalArgumentException("JTS adapter supports EvenOdd only");
                    }
                    runCase(id, operation, region(subjectPaths), region(clipPaths));
                } catch (RuntimeException error) {
                    printError(id, error.getMessage());
                }
            }
        }
    }

    private static List<List<Coordinate>> readPaths(Scanner input, int pathCount) {
        List<List<Coordinate>> paths = new ArrayList<>(pathCount);
        for (int pathIndex = 0; pathIndex < pathCount; pathIndex++) {
            int pointCount = input.nextInt();
            List<Coordinate> path = new ArrayList<>(pointCount);
            for (int pointIndex = 0; pointIndex < pointCount; pointIndex++) {
                path.add(new Coordinate(input.nextDouble(), input.nextDouble()));
            }
            paths.add(path);
        }
        return paths;
    }

    private static Geometry region(List<List<Coordinate>> paths) {
        List<Geometry> polygons = new ArrayList<>(paths.size());
        for (List<Coordinate> path : paths) {
            if (path.size() < 3) {
                continue;
            }
            Coordinate[] ring = new Coordinate[path.size() + 1];
            for (int index = 0; index < path.size(); index++) {
                ring[index] = path.get(index);
            }
            ring[path.size()] = new Coordinate(path.get(0));
            Geometry fixed = GeometryFixer.fix(FACTORY.createPolygon(ring));
            if (!fixed.isEmpty()) {
                polygons.add(fixed);
            }
        }
        return polygons.isEmpty()
                ? FACTORY.createPolygon()
                : OverlayNGRobust.union(polygons, FACTORY);
    }

    private static void runCase(String id, int operation, Geometry subjects, Geometry clips) {
        int overlayOperation = overlayOperation(operation);
        Geometry output = FACTORY.createPolygon();
        for (int run = 0; run < WARMUP_RUNS; run++) {
            output = OverlayNGRobust.overlay(subjects, clips, overlayOperation);
        }

        int iterationsPerSample = 1;
        while (true) {
            long started = System.nanoTime();
            for (int iteration = 0; iteration < iterationsPerSample; iteration++) {
                output = OverlayNGRobust.overlay(subjects, clips, overlayOperation);
            }
            long elapsed = System.nanoTime() - started;
            if (elapsed >= MINIMUM_SAMPLE_TIME_NS
                    || iterationsPerSample == MAXIMUM_ITERATIONS_PER_SAMPLE) {
                break;
            }
            iterationsPerSample = Math.min(
                    iterationsPerSample * 2, MAXIMUM_ITERATIONS_PER_SAMPLE);
        }

        long[] timings = new long[SAMPLE_RUNS];
        for (int run = 0; run < SAMPLE_RUNS; run++) {
            long started = System.nanoTime();
            for (int iteration = 0; iteration < iterationsPerSample; iteration++) {
                output = OverlayNGRobust.overlay(subjects, clips, overlayOperation);
            }
            timings[run] = (System.nanoTime() - started) / iterationsPerSample;
        }
        java.util.Arrays.sort(timings);

        String signature = signature(output);
        System.out.printf(
                Locale.ROOT,
                "{\"id\":\"%s\",\"status\":\"ok\",\"error\":null,\"median_ns\":%d,\"p95_ns\":%d,\"iterations_per_sample\":%d,\"ring_count\":%d,\"signature\":\"%s\"}%n",
                jsonEscape(id),
                timings[SAMPLE_RUNS / 2],
                timings[(SAMPLE_RUNS * 95 + 99) / 100 - 1],
                iterationsPerSample,
                rings(output).size(),
                jsonEscape(signature));
    }

    private static int overlayOperation(int operation) {
        return switch (operation) {
            case 0 -> OverlayNG.INTERSECTION;
            case 1 -> OverlayNG.UNION;
            case 2 -> OverlayNG.DIFFERENCE;
            case 3 -> OverlayNG.SYMDIFFERENCE;
            default -> throw new IllegalArgumentException("unknown operation " + operation);
        };
    }

    private static List<List<Coordinate>> rings(Geometry geometry) {
        List<List<Coordinate>> result = new ArrayList<>();
        collectRings(geometry, result);
        return result;
    }

    private static void collectRings(Geometry geometry, List<List<Coordinate>> result) {
        if (geometry instanceof Polygon polygon) {
            result.add(openRing(polygon.getExteriorRing().getCoordinates()));
            for (int index = 0; index < polygon.getNumInteriorRing(); index++) {
                result.add(openRing(polygon.getInteriorRingN(index).getCoordinates()));
            }
        } else if (geometry instanceof GeometryCollection collection) {
            for (int index = 0; index < collection.getNumGeometries(); index++) {
                collectRings(collection.getGeometryN(index), result);
            }
        }
    }

    private static List<Coordinate> openRing(Coordinate[] coordinates) {
        int length = coordinates.length;
        if (length > 1 && coordinates[0].equals2D(coordinates[length - 1])) {
            length--;
        }
        List<Coordinate> result = new ArrayList<>(length);
        for (int index = 0; index < length; index++) {
            result.add(new Coordinate(coordinates[index]));
        }
        return result;
    }

    private static String signature(Geometry geometry) {
        List<List<Coordinate>> rawRings = rings(geometry);
        StringBuilder output = new StringBuilder("[");
        for (int ringIndex = 0; ringIndex < rawRings.size(); ringIndex++) {
            if (ringIndex != 0) {
                output.append(',');
            }
            output.append('[');
            List<Coordinate> ring = rawRings.get(ringIndex);
            for (int pointIndex = 0; pointIndex < ring.size(); pointIndex++) {
                if (pointIndex != 0) {
                    output.append(',');
                }
                Coordinate point = ring.get(pointIndex);
                output.append('[').append(Double.toString(point.x))
                        .append(',').append(Double.toString(point.y)).append(']');
            }
            output.append(']');
        }
        return output.append(']').toString();
    }

    private static String jsonEscape(String value) {
        if (value == null) {
            return "unknown error";
        }
        return value.replace("\\", "\\\\")
                .replace("\"", "\\\"")
                .replace("\n", "\\n")
                .replace("\r", "\\r");
    }

    private static void printError(String id, String message) {
        System.out.printf(
                Locale.ROOT,
                "{\"id\":\"%s\",\"status\":\"error\",\"error\":\"%s\",\"median_ns\":0,\"p95_ns\":0,\"iterations_per_sample\":0,\"ring_count\":0,\"signature\":\"[]\"}%n",
                jsonEscape(id),
                jsonEscape(message));
    }
}
