import io.aeron.driver.MediaDriver;
import io.aeron.driver.ThreadingMode;

public class MediaDriverLauncher {
    public static void main(String[] args) throws Exception {
        System.out.println("Starting Aeron Media Driver...");

        MediaDriver.Context ctx = new MediaDriver.Context()
            .aeronDirectoryName("/dev/shm/aeron")
            .threadingMode(ThreadingMode.SHARED)
            .ipcTermBufferLength(1048576)
            .publicationTermBufferLength(1048576)
            .termBufferSparseFile(false)
            .performStorageChecks(false)
            .dirDeleteOnStart(true)
            .dirDeleteOnShutdown(false);

        MediaDriver driver = MediaDriver.launch(ctx);

        System.out.println("Aeron Media Driver started successfully");
        System.out.println("Aeron directory: " + driver.aeronDirectoryName());

        // Add shutdown hook
        Runtime.getRuntime().addShutdownHook(new Thread(() -> {
            System.out.println("Shutting down Media Driver...");
            driver.close();
        }));

        // Keep running until interrupted
        Thread.currentThread().join();
    }
}
