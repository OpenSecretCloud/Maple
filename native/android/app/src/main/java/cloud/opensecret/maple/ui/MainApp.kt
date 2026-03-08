package cloud.opensecret.maple.ui

import androidx.compose.animation.AnimatedContent
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.tween
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.interaction.collectIsPressedAsState
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.Send
import androidx.compose.material.icons.automirrored.filled.ExitToApp
import androidx.compose.material.icons.filled.Delete
import androidx.compose.material.icons.filled.Settings
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.scale
import androidx.compose.ui.draw.shadow
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import cloud.opensecret.maple.AppManager
import cloud.opensecret.maple.rust.AppAction
import cloud.opensecret.maple.rust.AuthState
import cloud.opensecret.maple.rust.ChatMessage
import cloud.opensecret.maple.rust.OAuthProvider
import cloud.opensecret.maple.rust.Screen
import cloud.opensecret.maple.ui.theme.*

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun MainApp(manager: AppManager) {
    val state = manager.state
    var showSplash by remember { mutableStateOf(true) }
    var minTimePassed by remember { mutableStateOf(false) }
    val currentScreen = state.router.defaultScreen

    LaunchedEffect(Unit) {
        kotlinx.coroutines.delay(1000)
        minTimePassed = true
    }

    LaunchedEffect(minTimePassed, currentScreen) {
        if (minTimePassed && currentScreen != Screen.LOADING) {
            showSplash = false
        }
    }

    Box(modifier = Modifier.fillMaxSize()) {
        when (state.router.defaultScreen) {
            Screen.LOADING -> SplashScreen()
            Screen.LOGIN -> LoginScreen(manager)
            Screen.CHAT -> AgentChatScreen(manager)
        }

        androidx.compose.animation.AnimatedVisibility(
            visible = showSplash,
            exit = androidx.compose.animation.fadeOut(
                animationSpec = androidx.compose.animation.core.tween(400),
            ),
        ) {
            SplashScreen()
        }
    }
}

// -- Splash --

@Composable
fun SplashScreen() {
    BoxWithConstraints(
        modifier = Modifier.fillMaxSize(),
        contentAlignment = Alignment.Center,
    ) {
        val w = constraints.maxWidth.toFloat()
        val h = constraints.maxHeight.toFloat()
        Box(
            modifier = Modifier
                .fillMaxSize()
                .background(
                    brush = Brush.radialGradient(
                        colors = listOf(
                            androidx.compose.ui.graphics.Color(0xFFFF9771),
                            androidx.compose.ui.graphics.Color(0xFFCE9A8E),
                            androidx.compose.ui.graphics.Color(0xFF9C9DAB),
                        ),
                        center = androidx.compose.ui.geometry.Offset(w / 2f, h),
                        radius = h,
                    ),
                ),
        )
        Column(horizontalAlignment = Alignment.CenterHorizontally) {
            Icon(
                painter = androidx.compose.ui.res.painterResource(id = cloud.opensecret.maple.R.drawable.maple_wordmark),
                contentDescription = "Maple",
                modifier = Modifier.height(40.dp),
                tint = Pebble50,
            )
            Spacer(modifier = Modifier.height(20.dp))
            Text(
                "Privacy-first intelligence",
                style = MaterialTheme.typography.bodyMedium.copy(
                    fontFamily = cloud.opensecret.maple.ui.theme.ArrayFamily,
                ),
                color = Pebble100.copy(alpha = 0.8f),
            )
        }
    }
}

// -- Login --

@Composable
fun LoginScreen(manager: AppManager) {
    val state = manager.state
    var email by remember { mutableStateOf("") }
    var password by remember { mutableStateOf("") }
    var name by remember { mutableStateOf("") }
    var isSignUp by remember { mutableStateOf(false) }

    val isLoading = state.auth is AuthState.LoggingIn || state.auth is AuthState.SigningUp

    val nameFocus = remember { FocusRequester() }
    val emailFocus = remember { FocusRequester() }
    val passwordFocus = remember { FocusRequester() }

    fun submitForm() {
        if (isSignUp) {
            manager.dispatch(AppAction.SignUpWithEmail(email = email, password = password, name = name))
        } else {
            manager.dispatch(AppAction.LoginWithEmail(email = email, password = password))
        }
    }

    BoxWithConstraints(
        modifier = Modifier.fillMaxSize(),
        contentAlignment = Alignment.Center,
    ) {
        val w = constraints.maxWidth.toFloat()
        val h = constraints.maxHeight.toFloat()
        Box(
            modifier = Modifier
                .fillMaxSize()
                .background(
                    brush = Brush.radialGradient(
                        colors = listOf(
                            Maple500.copy(alpha = 0.08f),
                            Bark300.copy(alpha = 0.06f),
                            Pebble400.copy(alpha = 0.04f),
                            Neutral50,
                        ),
                        center = androidx.compose.ui.geometry.Offset(w / 2f, h * 0.6f),
                        radius = h * 0.8f,
                    ),
                ),
        )
        Column(
            modifier = Modifier
                .widthIn(max = 360.dp)
                .padding(horizontal = 32.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.spacedBy(16.dp),
        ) {
            Icon(
                painter = androidx.compose.ui.res.painterResource(id = cloud.opensecret.maple.R.drawable.maple_wordmark),
                contentDescription = "Maple",
                modifier = Modifier.height(28.dp),
                tint = Neutral900,
            )

            if (isSignUp) {
                OutlinedTextField(
                    value = name,
                    onValueChange = { name = it },
                    label = { Text("Name") },
                    modifier = Modifier.fillMaxWidth().focusRequester(nameFocus),
                    singleLine = true,
                    shape = RoundedCornerShape(12.dp),
                    keyboardOptions = KeyboardOptions(imeAction = ImeAction.Next),
                    keyboardActions = KeyboardActions(onNext = { emailFocus.requestFocus() }),
                )
            }

            OutlinedTextField(
                value = email,
                onValueChange = { email = it },
                label = { Text("Email") },
                modifier = Modifier.fillMaxWidth().focusRequester(emailFocus),
                singleLine = true,
                shape = RoundedCornerShape(12.dp),
                keyboardOptions = KeyboardOptions(imeAction = ImeAction.Next),
                keyboardActions = KeyboardActions(onNext = { passwordFocus.requestFocus() }),
            )

            OutlinedTextField(
                value = password,
                onValueChange = { password = it },
                label = { Text("Password") },
                modifier = Modifier.fillMaxWidth().focusRequester(passwordFocus),
                singleLine = true,
                shape = RoundedCornerShape(12.dp),
                visualTransformation = PasswordVisualTransformation(),
                keyboardOptions = KeyboardOptions(imeAction = ImeAction.Go),
                keyboardActions = KeyboardActions(onGo = { submitForm() }),
            )

            val primaryInteractionSource = remember { MutableInteractionSource() }
            val primaryPressed by primaryInteractionSource.collectIsPressedAsState()
            val primaryScale by animateFloatAsState(
                targetValue = if (primaryPressed) 0.95f else 1f,
                animationSpec = tween(durationMillis = 200),
            )

            Button(
                onClick = { submitForm() },
                modifier = Modifier.fillMaxWidth().height(48.dp).scale(primaryScale),
                enabled = email.isNotEmpty() && password.isNotEmpty() && !isLoading,
                interactionSource = primaryInteractionSource,
                shape = RoundedCornerShape(999.dp),
                colors = ButtonDefaults.buttonColors(
                    containerColor = Maple500,
                    contentColor = androidx.compose.ui.graphics.Color.White,
                    disabledContainerColor = Maple200,
                    disabledContentColor = androidx.compose.ui.graphics.Color.White,
                ),
            ) {
                if (isLoading) {
                    CircularProgressIndicator(
                        modifier = Modifier.size(20.dp),
                        strokeWidth = 2.dp,
                        color = androidx.compose.ui.graphics.Color.White,
                    )
                } else {
                    Text(
                        if (isSignUp) "Sign Up" else "Sign In",
                        fontWeight = FontWeight.SemiBold,
                    )
                }
            }

            Row(
                modifier = Modifier.fillMaxWidth(),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                HorizontalDivider(modifier = Modifier.weight(1f), color = Neutral200)
                Text("or", style = MaterialTheme.typography.labelSmall, color = Pebble400)
                HorizontalDivider(modifier = Modifier.weight(1f), color = Neutral200)
            }

            Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                GlassyOutlinedButton(
                    text = "Continue with GitHub",
                    onClick = { manager.dispatch(AppAction.InitiateOAuth(provider = OAuthProvider.GITHUB, inviteCode = null)) },
                    enabled = !isLoading,
                )
                GlassyOutlinedButton(
                    text = "Continue with Google",
                    onClick = { manager.dispatch(AppAction.InitiateOAuth(provider = OAuthProvider.GOOGLE, inviteCode = null)) },
                    enabled = !isLoading,
                )
                GlassyOutlinedButton(
                    text = "Continue with Apple",
                    onClick = { manager.dispatch(AppAction.InitiateOAuth(provider = OAuthProvider.APPLE, inviteCode = null)) },
                    enabled = !isLoading,
                )
            }

            TextButton(onClick = { isSignUp = !isSignUp }) {
                Text(
                    if (isSignUp) "Already have an account? Sign In" else "Don't have an account? Sign Up",
                    color = Pebble500,
                )
            }

            state.toast?.let { toast ->
                Text(toast, color = MapleError, style = MaterialTheme.typography.bodySmall)
            }
        }
    }
}

@Composable
private fun GlassyOutlinedButton(
    text: String,
    onClick: () -> Unit,
    enabled: Boolean = true,
) {
    val interactionSource = remember { MutableInteractionSource() }
    val isPressed by interactionSource.collectIsPressedAsState()
    val scale by animateFloatAsState(
        targetValue = if (isPressed) 0.95f else 1f,
        animationSpec = tween(durationMillis = 200),
    )

    Button(
        onClick = onClick,
        modifier = Modifier.fillMaxWidth().height(44.dp).scale(scale),
        enabled = enabled,
        interactionSource = interactionSource,
        shape = RoundedCornerShape(999.dp),
        colors = ButtonDefaults.buttonColors(
            containerColor = Pebble100.copy(alpha = 0.4f),
            contentColor = Pebble700,
            disabledContainerColor = Pebble100.copy(alpha = 0.2f),
            disabledContentColor = Pebble400,
        ),
        elevation = ButtonDefaults.buttonElevation(
            defaultElevation = 0.dp,
            pressedElevation = 0.dp,
        ),
    ) { Text(text) }
}

// -- Agent Chat --

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun AgentChatScreen(manager: AppManager) {
    val state = manager.state
    var composeText by remember { mutableStateOf("") }
    val listState = rememberLazyListState()

    // Refresh relative timestamps every 30s
    LaunchedEffect(Unit) {
        while (true) {
            kotlinx.coroutines.delay(30_000)
            manager.dispatch(AppAction.RefreshTimestamps)
        }
    }

    // Prefetch older messages when scrolled near top
    val firstVisibleIndex by remember { derivedStateOf { listState.firstVisibleItemIndex } }
    LaunchedEffect(firstVisibleIndex) {
        // Account for load-more item at index 0; messages start at index 1 when it's visible
        if (firstVisibleIndex <= 3 && state.hasOlderMessages && !state.isLoadingHistory) {
            manager.dispatch(AppAction.LoadOlderMessages)
        }
    }

    // Auto-scroll to bottom (including typing indicator) when new content arrives
    var prevMessageCount by remember { mutableIntStateOf(state.messages.size) }
    LaunchedEffect(state.messages.size, state.messages.lastOrNull()?.content, state.isAgentTyping) {
        val totalItems = listState.layoutInfo.totalItemsCount
        if (totalItems > 0 && state.messages.size >= prevMessageCount) {
            listState.animateScrollToItem(totalItems - 1)
        }
        prevMessageCount = state.messages.size
    }

    BoxWithConstraints(modifier = Modifier.fillMaxSize()) {
        val w = constraints.maxWidth.toFloat()
        val h = constraints.maxHeight.toFloat()

        // Background mesh gradient
        Box(
            modifier = Modifier
                .fillMaxSize()
                .background(
                    brush = Brush.radialGradient(
                        colors = listOf(
                            Pebble100,
                            Maple50,
                            Bark50,
                            Neutral0,
                        ),
                        center = androidx.compose.ui.geometry.Offset(w * 0.3f, h * 0.4f),
                        radius = h * 0.9f,
                    ),
                ),
        )

        // Messages list (full size, scrolls behind header and compose bar)
        LazyColumn(
            modifier = Modifier.fillMaxSize(),
            state = listState,
            contentPadding = PaddingValues(
                start = 16.dp, end = 16.dp,
                top = 64.dp, bottom = 104.dp,
            ),
            verticalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            if (state.isLoadingHistory) {
                item(key = "loading-history") {
                    Box(modifier = Modifier.fillMaxWidth().padding(vertical = 8.dp), contentAlignment = Alignment.Center) {
                        CircularProgressIndicator(modifier = Modifier.size(20.dp), strokeWidth = 2.dp)
                    }
                }
            } else if (state.hasOlderMessages) {
                item(key = "load-more") {
                    Box(modifier = Modifier.fillMaxWidth().padding(vertical = 8.dp), contentAlignment = Alignment.Center) {
                        TextButton(onClick = { manager.dispatch(AppAction.LoadOlderMessages) }) {
                            Text("Load older messages", color = Maple500, style = MaterialTheme.typography.bodySmall)
                        }
                    }
                }
            }

            items(state.messages, key = { it.id }) { message ->
                MessageBubble(message)
            }

            if (state.isAgentTyping) {
                item {
                    Text(
                        "Maple is typing...",
                        style = MaterialTheme.typography.bodySmall,
                        color = Pebble400,
                        modifier = Modifier.padding(start = 8.dp),
                    )
                }
            }
        }

        // Toast (above compose bar)
        state.toast?.let { toast ->
            Text(
                toast,
                color = MapleError,
                style = MaterialTheme.typography.bodySmall,
                modifier = Modifier
                    .align(Alignment.BottomCenter)
                    .padding(bottom = 80.dp, start = 16.dp, end = 16.dp),
            )
        }

        // Floating header islands
        Box(
            modifier = Modifier
                .fillMaxWidth()
                .statusBarsPadding()
                .padding(horizontal = 16.dp, vertical = 8.dp)
                .align(Alignment.TopCenter),
        ) {
            // Wordmark pill (centered)
            Box(
                modifier = Modifier
                    .align(Alignment.Center)
                    .shadow(2.dp, RoundedCornerShape(999.dp))
                    .background(Neutral0.copy(alpha = 0.7f), RoundedCornerShape(999.dp))
                    .border(0.5.dp, Pebble200.copy(alpha = 0.3f), RoundedCornerShape(999.dp))
                    .padding(horizontal = 20.dp, vertical = 10.dp),
            ) {
                Icon(
                    painter = androidx.compose.ui.res.painterResource(id = cloud.opensecret.maple.R.drawable.maple_wordmark),
                    contentDescription = "Maple",
                    modifier = Modifier.height(20.dp),
                    tint = Pebble700,
                )
            }

            // Gear pill (end aligned)
            Box(
                modifier = Modifier
                    .align(Alignment.CenterEnd)
                    .size(40.dp)
                    .shadow(2.dp, RoundedCornerShape(999.dp))
                    .background(Neutral0.copy(alpha = 0.7f), RoundedCornerShape(999.dp))
                    .border(0.5.dp, Pebble200.copy(alpha = 0.3f), RoundedCornerShape(999.dp)),
                contentAlignment = Alignment.Center,
            ) {
                IconButton(
                    onClick = { manager.dispatch(AppAction.ToggleSettings) },
                    modifier = Modifier.size(40.dp),
                ) {
                    Icon(
                        imageVector = Icons.Filled.Settings,
                        contentDescription = "Settings",
                        tint = Pebble500,
                    )
                }
            }
        }

        // Floating compose bar
        Row(
            modifier = Modifier
                .align(Alignment.BottomCenter)
                .fillMaxWidth()
                .navigationBarsPadding()
                .padding(horizontal = 16.dp, vertical = 8.dp)
                .shadow(2.dp, RoundedCornerShape(999.dp))
                .background(Neutral0.copy(alpha = 0.7f), RoundedCornerShape(999.dp))
                .border(0.5.dp, Pebble200.copy(alpha = 0.3f), RoundedCornerShape(999.dp))
                .padding(horizontal = 12.dp, vertical = 6.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            val composeFocus = remember { FocusRequester() }

            OutlinedTextField(
                value = composeText,
                onValueChange = { composeText = it },
                placeholder = { Text("Message Maple...", color = Pebble400) },
                modifier = Modifier.weight(1f).focusRequester(composeFocus),
                singleLine = true,
                shape = RoundedCornerShape(999.dp),
                keyboardOptions = KeyboardOptions(imeAction = ImeAction.Send),
                keyboardActions = KeyboardActions(onSend = {
                    val text = composeText.trim()
                    if (text.isNotEmpty() && !state.isAgentTyping) {
                        manager.dispatch(AppAction.SendMessage(content = text))
                        composeText = ""
                    }
                }),
            )

            IconButton(
                onClick = {
                    val text = composeText.trim()
                    if (text.isNotEmpty()) {
                        manager.dispatch(AppAction.SendMessage(content = text))
                        composeText = ""
                    }
                },
                enabled = composeText.trim().isNotEmpty() && !state.isAgentTyping,
                colors = IconButtonDefaults.iconButtonColors(
                    contentColor = Maple500,
                    disabledContentColor = Neutral300,
                ),
            ) {
                Icon(Icons.AutoMirrored.Filled.Send, contentDescription = "Send")
            }
        }
    }

    if (state.showSettings) {
        SettingsSheet(manager = manager)
    }

    if (state.confirmDeleteAgent) {
        AlertDialog(
            onDismissRequest = { manager.dispatch(AppAction.CancelDeleteAgent) },
            title = { Text("Delete Agent?") },
            text = { Text("This will permanently delete your agent conversation history. This cannot be undone.") },
            confirmButton = {
                TextButton(onClick = { manager.dispatch(AppAction.ConfirmDeleteAgent) }) {
                    Text("Delete", color = MapleError)
                }
            },
            dismissButton = {
                TextButton(onClick = { manager.dispatch(AppAction.CancelDeleteAgent) }) {
                    Text("Cancel")
                }
            },
        )
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun SettingsSheet(manager: AppManager) {
    val sheetState = rememberModalBottomSheetState()

    ModalBottomSheet(
        onDismissRequest = { manager.dispatch(AppAction.ToggleSettings) },
        sheetState = sheetState,
    ) {
        Column(modifier = Modifier.padding(horizontal = 16.dp, vertical = 8.dp)) {
            Text(
                "Settings",
                style = MaterialTheme.typography.titleMedium,
                modifier = Modifier.padding(bottom = 16.dp),
            )
            TextButton(
                onClick = { manager.dispatch(AppAction.RequestDeleteAgent) },
                enabled = !manager.state.isDeletingAgent,
                shape = RoundedCornerShape(999.dp),
            ) {
                Icon(
                    imageVector = Icons.Filled.Delete,
                    contentDescription = null,
                    tint = MapleError,
                    modifier = Modifier.padding(end = 8.dp),
                )
                Text("Delete Agent", color = MapleError)
            }
            HorizontalDivider(modifier = Modifier.padding(vertical = 8.dp))
            TextButton(
                onClick = {
                    manager.dispatch(AppAction.ToggleSettings)
                    manager.dispatch(AppAction.Logout)
                },
                shape = RoundedCornerShape(999.dp),
            ) {
                Icon(
                    imageVector = Icons.AutoMirrored.Filled.ExitToApp,
                    contentDescription = null,
                    tint = Pebble500,
                    modifier = Modifier.padding(end = 8.dp),
                )
                Text("Sign Out", color = Neutral800)
            }
            Spacer(modifier = Modifier.height(32.dp))
        }
    }
}

@Composable
fun MessageBubble(message: ChatMessage) {
    val isUser = message.isUser
    val alignment = if (isUser) Alignment.End else Alignment.Start

    Column(
        modifier = Modifier.fillMaxWidth(),
        horizontalAlignment = alignment,
    ) {
        if (!isUser && message.showSender) {
            Text(
                "Maple",
                style = MaterialTheme.typography.labelSmall,
                color = Pebble400,
                modifier = Modifier.padding(start = 8.dp, bottom = 2.dp),
            )
        }
        Box(
            modifier = Modifier
                .widthIn(max = 300.dp)
                .then(
                    if (isUser) {
                        Modifier.background(
                            brush = Brush.verticalGradient(
                                colors = listOf(Maple400, Maple600),
                            ),
                            shape = RoundedCornerShape(
                                topStart = 16.dp,
                                topEnd = 16.dp,
                                bottomStart = 16.dp,
                                bottomEnd = 4.dp,
                            ),
                        )
                    } else {
                        Modifier.background(
                            Pebble50,
                            RoundedCornerShape(
                                topStart = 16.dp,
                                topEnd = 16.dp,
                                bottomStart = 4.dp,
                                bottomEnd = 16.dp,
                            ),
                        )
                    }
                )
                .padding(horizontal = 12.dp, vertical = 8.dp),
        ) {
            Text(
                text = message.content,
                color = if (isUser) androidx.compose.ui.graphics.Color.White else Neutral800,
                style = MaterialTheme.typography.bodyMedium,
            )
        }
        if (message.showTimestamp) {
            Text(
                text = message.timestampDisplay,
                style = MaterialTheme.typography.labelSmall.copy(fontSize = 10.sp),
                color = Pebble400,
                modifier = Modifier.padding(start = 8.dp, end = 8.dp).padding(top = 2.dp),
            )
        }
    }
}
