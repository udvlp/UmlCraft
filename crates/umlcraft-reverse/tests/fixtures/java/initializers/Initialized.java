package initializers;

import java.util.ArrayList;
import java.util.List;

public class Initialized {
    private int count = 1 + 2;
    private String name = "none";
    private List<String> names = new ArrayList<>();
    private int[] numbers = {1, 2, 3};
    private Runnable task = new Runnable() {
        public void run() {
        }
    };

    static {
        System.out.println("loaded");
    }

    {
        count++;
    }

    void work() {
        Runnable r = () -> {
        };
        class Helper {
        }
        new Object() {
        };
    }
}
