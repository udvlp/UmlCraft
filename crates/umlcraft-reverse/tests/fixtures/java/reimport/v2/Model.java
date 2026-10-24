package model;

/** Now documented. */
public class Account implements Missing {
    private long balance;
    private Owner owner;
    private String note;

    public void deposit(int amount) {
    }

    public void deposit(long amount) {
    }
}

class Base {
}

class Owner {
}

interface Missing {
}
