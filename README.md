# ConnectFour 3D

This is my first ever Rust code. It's just a toy project that I implemented in
order to complement my theoretical knowledge from the book with some practice.

There is a board game called "Connect Four 3D", in real life it looks like this:

![image](res/irl.jpg)

It happened that I and my relatives played it a lot, and I wanted to have a
computer version. There are some on the internet, but none that I know of
supports network multiplayer. So here we go, I created my own version of it,
with the network multiplayer supported.

## Running

To run in network mode, connecting to the default server, to a game called
`mygame`, clone it and run:

```
cargo run --bin connectfour-3d -- --game mygame
```

Once there are two players connected with the same game ID (in this case,
`mygame`), the game will start.

## Demo

![image](res/connectfour-3d.gif)
