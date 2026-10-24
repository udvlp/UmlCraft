package model;

public class Account extends Base {
    private int balance;
    private Owner owner;
    private Owner former;

    public void deposit(int amount) {
    }

    public void close() {
    }
}

class Base {
}

class Owner {
}

class Placeholder extends Missing {
}
