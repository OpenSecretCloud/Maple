package cloud.opensecret.maple.ui

import androidx.compose.animation.AnimatedContent
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.slideInVertically
import androidx.compose.animation.slideOutVertically
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.tween
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.clickable
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.interaction.collectIsPressedAsState
import androidx.compose.foundation.isSystemInDarkTheme
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
import androidx.compose.material.icons.filled.Menu
import androidx.compose.material.icons.filled.Search
import androidx.compose.material.icons.filled.Settings
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.scale
import androidx.compose.ui.draw.shadow
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.layout.positionInRoot
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.text.input.VisualTransformation
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
import kotlin.math.roundToInt

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun MainApp(manager: AppManager) {
    val state = manager.state
    var showSplash by remember { mutableStateOf(true) }
    var minTimePassed by remember { mutableStateOf(false) }
    val currentScreen = state.router.defaultScreen
    val toastMessage = state.toast

    LaunchedEffect(Unit) {
        kotlinx.coroutines.delay(1000)
        minTimePassed = true
    }

    LaunchedEffect(minTimePassed, currentScreen) {
        if (minTimePassed && currentScreen != Screen.LOADING) {
            showSplash = false
        }
    }

    LaunchedEffect(toastMessage) {
        if (toastMessage == null) return@LaunchedEffect
        kotlinx.coroutines.delay(4000)
        if (manager.state.toast == toastMessage) {
            manager.dispatch(AppAction.ClearToast)
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

        MapleToastHost(
            message = toastMessage,
            isChatScreen = currentScreen == Screen.CHAT,
            onDismiss = { manager.dispatch(AppAction.ClearToast) },
        )
    }
}

@Composable
private fun MapleToastHost(
    message: String?,
    isChatScreen: Boolean,
    onDismiss: () -> Unit,
) {
    val isDarkTheme = isSystemInDarkTheme()

    AnimatedVisibility(
        visible = message != null,
        modifier = Modifier.fillMaxSize(),
        enter = fadeIn(animationSpec = tween(durationMillis = 180)) +
            slideInVertically(animationSpec = tween(durationMillis = 220)) { it / 2 },
        exit = fadeOut(animationSpec = tween(durationMillis = 160)) +
            slideOutVertically(animationSpec = tween(durationMillis = 200)) { it / 2 },
    ) {
        Box(
            modifier = Modifier.fillMaxSize(),
            contentAlignment = Alignment.BottomCenter,
        ) {
            Surface(
                modifier = Modifier
                    .padding(horizontal = 16.dp)
                    .padding(bottom = if (isChatScreen) 112.dp else 24.dp)
                    .clickable(onClick = onDismiss),
                shape = RoundedCornerShape(999.dp),
                color = if (isDarkTheme) {
                    Color(0xFF271D1A).copy(alpha = 0.92f)
                } else {
                    Color.White.copy(alpha = 0.94f)
                },
                tonalElevation = 0.dp,
                shadowElevation = if (isDarkTheme) 8.dp else 4.dp,
                border = BorderStroke(
                    1.dp,
                    MapleError.copy(alpha = if (isDarkTheme) 0.35f else 0.18f),
                ),
            ) {
                Text(
                    text = message.orEmpty(),
                    color = if (isDarkTheme) Pebble50 else Neutral800,
                    style = MaterialTheme.typography.bodySmall,
                    modifier = Modifier.padding(horizontal = 16.dp, vertical = 12.dp),
                )
            }
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

private data class LoginPalette(
    val backgroundBase: Color,
    val backgroundGlow: List<Color>,
    val cardBackground: Color,
    val cardHighlight: Color,
    val cardBorder: Color,
    val cardShadow: Color,
    val wordmark: Color,
    val supportingText: Color,
    val tertiaryText: Color,
    val divider: Color,
    val fieldBackground: Color,
    val fieldBorder: Color,
    val fieldText: Color,
    val fieldPlaceholder: Color,
    val secondaryButtonBackground: Color,
    val secondaryButtonBorder: Color,
    val secondaryButtonForeground: Color,
    val secondaryButtonShadow: Color,
)

private fun loginPalette(isDarkTheme: Boolean): LoginPalette =
    if (isDarkTheme) {
        LoginPalette(
            backgroundBase = Color(0xFF1A110E),
            backgroundGlow = listOf(
                Maple500.copy(alpha = 0.07f),
                Color(0xFF5D4036).copy(alpha = 0.11f),
                Pebble800.copy(alpha = 0.12f),
                Color.Transparent,
            ),
            cardBackground = Color(0xFF271D1A).copy(alpha = 0.9f),
            cardHighlight = Color.White.copy(alpha = 0.04f),
            cardBorder = Color(0xFF53433E),
            cardShadow = Color.Black.copy(alpha = 0.4f),
            wordmark = Pebble50,
            supportingText = Color(0xFFD8C2BB),
            tertiaryText = Color(0xFFD8C2BB).copy(alpha = 0.8f),
            divider = Color(0xFF53433E),
            fieldBackground = Color(0xFF231A16).copy(alpha = 0.96f),
            fieldBorder = Color(0xFFA08D86).copy(alpha = 0.45f),
            fieldText = Color(0xFFF1DFD9),
            fieldPlaceholder = Color(0xFFD8C2BB).copy(alpha = 0.75f),
            secondaryButtonBackground = Color(0xFF322824).copy(alpha = 0.96f),
            secondaryButtonBorder = Color(0xFF53433E),
            secondaryButtonForeground = Color(0xFFF1DFD9),
            secondaryButtonShadow = Color.Black.copy(alpha = 0.14f),
        )
    } else {
        LoginPalette(
            backgroundBase = Color(0xFFFBF8F6),
            backgroundGlow = listOf(
                Maple500.copy(alpha = 0.18f),
                Bark300.copy(alpha = 0.1f),
                Pebble300.copy(alpha = 0.1f),
                Color.Transparent,
            ),
            cardBackground = Color.White.copy(alpha = 0.74f),
            cardHighlight = Color.White.copy(alpha = 0.42f),
            cardBorder = Color.White.copy(alpha = 0.72f),
            cardShadow = Pebble900.copy(alpha = 0.08f),
            wordmark = Pebble800,
            supportingText = Pebble600,
            tertiaryText = Pebble400,
            divider = Neutral200,
            fieldBackground = Color.White.copy(alpha = 0.84f),
            fieldBorder = Neutral200.copy(alpha = 0.95f),
            fieldText = Pebble800,
            fieldPlaceholder = Pebble400,
            secondaryButtonBackground = Color.White.copy(alpha = 0.56f),
            secondaryButtonBorder = Color.White.copy(alpha = 0.68f),
            secondaryButtonForeground = Pebble700,
            secondaryButtonShadow = Pebble900.copy(alpha = 0.05f),
        )
    }

private data class ChatPalette(
    val backgroundBase: Color,
    val backgroundGlow: List<Color>,
    val chromeBackground: Color,
    val chromeBorder: Color,
    val headerWordmark: Color,
    val secondaryIcon: Color,
    val composeText: Color,
    val composePlaceholder: Color,
    val metadataText: Color,
    val assistantText: Color,
    val userBubbleColor: Color,
    val userText: Color,
    val surfaceText: Color,
    val sheetBackground: Color,
    val sheetDivider: Color,
    val sheetSecondaryText: Color,
    val disabledIcon: Color,
)

private fun chatPalette(isDarkTheme: Boolean): ChatPalette =
    if (isDarkTheme) {
        ChatPalette(
            backgroundBase = Color(0xFF1A110E),
            backgroundGlow = listOf(
                Maple500.copy(alpha = 0.07f),
                Color(0xFF5D4036).copy(alpha = 0.1f),
                Pebble800.copy(alpha = 0.12f),
                Color.Transparent,
            ),
            chromeBackground = Color(0xFF271D1A).copy(alpha = 0.78f),
            chromeBorder = Color(0xFF53433E),
            headerWordmark = Pebble50,
            secondaryIcon = Color(0xFFD8C2BB),
            composeText = Color(0xFFF1DFD9),
            composePlaceholder = Color(0xFFD8C2BB).copy(alpha = 0.78f),
            metadataText = Color(0xFFD8C2BB).copy(alpha = 0.82f),
            assistantText = Color(0xFFF1DFD9),
            userBubbleColor = Color(0xFF322824).copy(alpha = 0.96f),
            userText = Color(0xFFF1DFD9),
            surfaceText = Color(0xFFF1DFD9),
            sheetBackground = Color(0xFF271D1A),
            sheetDivider = Color(0xFF53433E),
            sheetSecondaryText = Color(0xFFD8C2BB),
            disabledIcon = Color(0xFFA08D86).copy(alpha = 0.45f),
        )
    } else {
        ChatPalette(
            backgroundBase = Color.White,
            backgroundGlow = listOf(
                Color(0xFFFF9771),
                Color(0xFFECB8A5),
                Color(0xFFDADADA),
                Color.White,
            ),
            chromeBackground = Color.White.copy(alpha = 0.4f),
            chromeBorder = Color.Transparent,
            headerWordmark = Pebble800,
            secondaryIcon = Pebble800,
            composeText = Neutral800,
            composePlaceholder = Color(0xFF878787),
            metadataText = Pebble400,
            assistantText = Pebble800,
            userBubbleColor = Pebble100,
            userText = Neutral800,
            surfaceText = Neutral800,
            sheetBackground = Color.White.copy(alpha = 0.96f),
            sheetDivider = Neutral200,
            sheetSecondaryText = Pebble500,
            disabledIcon = Neutral300,
        )
    }

@Composable
private fun composeFieldColors(palette: ChatPalette) = OutlinedTextFieldDefaults.colors(
    focusedContainerColor = Color.Transparent,
    unfocusedContainerColor = Color.Transparent,
    disabledContainerColor = Color.Transparent,
    focusedTextColor = palette.composeText,
    unfocusedTextColor = palette.composeText,
    disabledTextColor = palette.composeText.copy(alpha = 0.5f),
    cursorColor = Maple500,
    focusedBorderColor = Color.Transparent,
    unfocusedBorderColor = Color.Transparent,
    disabledBorderColor = Color.Transparent,
    focusedPlaceholderColor = palette.composePlaceholder,
    unfocusedPlaceholderColor = palette.composePlaceholder,
    disabledPlaceholderColor = palette.composePlaceholder.copy(alpha = 0.5f),
)

@Composable
fun LoginScreen(manager: AppManager) {
    val state = manager.state
    val isDarkTheme = isSystemInDarkTheme()
    val palette = loginPalette(isDarkTheme)
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

    val canSubmit = email.isNotEmpty() && password.isNotEmpty() && !isLoading

    BoxWithConstraints(
        modifier = Modifier.fillMaxSize(),
        contentAlignment = Alignment.Center,
    ) {
        val w = constraints.maxWidth.toFloat()
        val h = constraints.maxHeight.toFloat()
        Box(
            modifier = Modifier
                .fillMaxSize()
                .background(palette.backgroundBase),
        )
        Box(
            modifier = Modifier
                .fillMaxSize()
                .background(
                    brush = Brush.radialGradient(
                        colors = palette.backgroundGlow,
                        center = androidx.compose.ui.geometry.Offset(w / 2f, h),
                        radius = h,
                    ),
                ),
        )
        Column(
            modifier = Modifier
                .fillMaxSize()
                .padding(horizontal = 20.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
        ) {
            Spacer(modifier = Modifier.weight(1f))

            Column(
                modifier = Modifier
                    .fillMaxWidth()
                    .widthIn(max = 360.dp)
                    .shadow(
                        elevation = if (isDarkTheme) 28.dp else 20.dp,
                        shape = RoundedCornerShape(24.dp),
                        ambientColor = palette.cardShadow,
                        spotColor = palette.cardShadow,
                    )
                    .clip(RoundedCornerShape(24.dp))
                    .background(
                        brush = Brush.linearGradient(
                            colors = listOf(palette.cardHighlight, palette.cardBackground),
                        ),
                    )
                    .border(1.dp, palette.cardBorder, RoundedCornerShape(24.dp))
                    .padding(24.dp),
                horizontalAlignment = Alignment.CenterHorizontally,
                verticalArrangement = Arrangement.spacedBy(24.dp),
            ) {
                Icon(
                    painter = androidx.compose.ui.res.painterResource(id = cloud.opensecret.maple.R.drawable.maple_wordmark),
                    contentDescription = "Maple",
                    modifier = Modifier.height(28.dp),
                    tint = palette.wordmark,
                )

                Column(
                    modifier = Modifier.fillMaxWidth(),
                    verticalArrangement = Arrangement.spacedBy(12.dp),
                ) {
                    if (isSignUp) {
                        MapleLoginTextField(
                            value = name,
                            onValueChange = { name = it },
                            placeholder = "Name",
                            palette = palette,
                            modifier = Modifier.focusRequester(nameFocus),
                            keyboardOptions = KeyboardOptions(imeAction = ImeAction.Next),
                            keyboardActions = KeyboardActions(onNext = { emailFocus.requestFocus() }),
                        )
                    }

                    MapleLoginTextField(
                        value = email,
                        onValueChange = { email = it },
                        placeholder = "Email",
                        palette = palette,
                        modifier = Modifier.focusRequester(emailFocus),
                        keyboardOptions = KeyboardOptions(imeAction = ImeAction.Next),
                        keyboardActions = KeyboardActions(onNext = { passwordFocus.requestFocus() }),
                    )

                    MapleLoginTextField(
                        value = password,
                        onValueChange = { password = it },
                        placeholder = "Password",
                        palette = palette,
                        modifier = Modifier.focusRequester(passwordFocus),
                        visualTransformation = PasswordVisualTransformation(),
                        keyboardOptions = KeyboardOptions(imeAction = ImeAction.Go),
                        keyboardActions = KeyboardActions(onGo = { submitForm() }),
                    )
                }

                val primaryInteractionSource = remember { MutableInteractionSource() }
                val primaryPressed by primaryInteractionSource.collectIsPressedAsState()
                val primaryScale by animateFloatAsState(
                    targetValue = if (primaryPressed) 0.95f else 1f,
                    animationSpec = tween(durationMillis = 200),
                )

                Button(
                    onClick = { submitForm() },
                    modifier = Modifier
                        .fillMaxWidth()
                        .height(48.dp)
                        .scale(primaryScale),
                    enabled = canSubmit,
                    interactionSource = primaryInteractionSource,
                    shape = RoundedCornerShape(999.dp),
                    contentPadding = PaddingValues(0.dp),
                    colors = ButtonDefaults.buttonColors(
                        containerColor = Color.Transparent,
                        contentColor = Color.White,
                        disabledContainerColor = Color.Transparent,
                        disabledContentColor = Color.White,
                    ),
                    elevation = ButtonDefaults.buttonElevation(
                        defaultElevation = 0.dp,
                        pressedElevation = 0.dp,
                        disabledElevation = 0.dp,
                    ),
                ) {
                    Box(
                        modifier = Modifier
                            .fillMaxSize()
                            .clip(RoundedCornerShape(999.dp))
                            .background(
                                brush = Brush.verticalGradient(
                                    colors = listOf(Maple400, Maple600),
                                ),
                            )
                            .alpha(if (canSubmit || isLoading) 1f else 0.5f),
                        contentAlignment = Alignment.Center,
                    ) {
                        if (isLoading) {
                            CircularProgressIndicator(
                                modifier = Modifier.size(20.dp),
                                strokeWidth = 2.dp,
                                color = Color.White,
                            )
                        } else {
                            Text(
                                if (isSignUp) "Sign Up" else "Sign In",
                                color = Color.White,
                                fontWeight = FontWeight.SemiBold,
                            )
                        }
                    }
                }

                Row(
                    modifier = Modifier.fillMaxWidth(),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(8.dp),
                ) {
                    HorizontalDivider(modifier = Modifier.weight(1f), color = palette.divider)
                    Text("or", style = MaterialTheme.typography.labelSmall, color = palette.tertiaryText)
                    HorizontalDivider(modifier = Modifier.weight(1f), color = palette.divider)
                }

                Column(
                    modifier = Modifier.fillMaxWidth(),
                    verticalArrangement = Arrangement.spacedBy(8.dp),
                ) {
                    GlassyOutlinedButton(
                        text = "Continue with GitHub",
                        onClick = { manager.dispatch(AppAction.InitiateOAuth(provider = OAuthProvider.GITHUB, inviteCode = null)) },
                        palette = palette,
                        enabled = !isLoading,
                    )
                    GlassyOutlinedButton(
                        text = "Continue with Google",
                        onClick = { manager.dispatch(AppAction.InitiateOAuth(provider = OAuthProvider.GOOGLE, inviteCode = null)) },
                        palette = palette,
                        enabled = !isLoading,
                    )
                    GlassyOutlinedButton(
                        text = "Continue with Apple",
                        onClick = { manager.dispatch(AppAction.InitiateOAuth(provider = OAuthProvider.APPLE, inviteCode = null)) },
                        palette = palette,
                        enabled = !isLoading,
                    )
                }

                TextButton(onClick = { isSignUp = !isSignUp }) {
                    Text(
                        if (isSignUp) "Already have an account? Sign In" else "Don't have an account? Sign Up",
                        color = palette.supportingText,
                    )
                }
            }

            Spacer(modifier = Modifier.weight(1f))
        }
    }
}

@Composable
private fun MapleLoginTextField(
    value: String,
    onValueChange: (String) -> Unit,
    placeholder: String,
    palette: LoginPalette,
    modifier: Modifier = Modifier,
    keyboardOptions: KeyboardOptions = KeyboardOptions.Default,
    keyboardActions: KeyboardActions = KeyboardActions.Default,
    visualTransformation: VisualTransformation = VisualTransformation.None,
) {
    val shape = RoundedCornerShape(12.dp)

    BasicTextField(
        value = value,
        onValueChange = onValueChange,
        modifier = modifier
            .fillMaxWidth()
            .clip(shape)
            .background(palette.fieldBackground, shape)
            .border(1.dp, palette.fieldBorder, shape)
            .padding(horizontal = 16.dp, vertical = 14.dp),
        textStyle = MaterialTheme.typography.bodyLarge.copy(color = palette.fieldText),
        singleLine = true,
        keyboardOptions = keyboardOptions,
        keyboardActions = keyboardActions,
        visualTransformation = visualTransformation,
        cursorBrush = SolidColor(Maple500),
        decorationBox = { innerTextField ->
            Box(modifier = Modifier.fillMaxWidth()) {
                if (value.isEmpty()) {
                    Text(
                        placeholder,
                        style = MaterialTheme.typography.bodyLarge,
                        color = palette.fieldPlaceholder,
                    )
                }
                innerTextField()
            }
        },
    )
}

@Composable
private fun GlassyOutlinedButton(
    text: String,
    onClick: () -> Unit,
    palette: LoginPalette,
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
        modifier = Modifier
            .fillMaxWidth()
            .height(44.dp)
            .scale(scale)
            .shadow(
                elevation = 10.dp,
                shape = RoundedCornerShape(999.dp),
                ambientColor = palette.secondaryButtonShadow,
                spotColor = palette.secondaryButtonShadow,
            ),
        enabled = enabled,
        interactionSource = interactionSource,
        shape = RoundedCornerShape(999.dp),
        border = BorderStroke(
            1.dp,
            if (enabled) palette.secondaryButtonBorder else palette.secondaryButtonBorder.copy(alpha = 0.5f),
        ),
        colors = ButtonDefaults.buttonColors(
            containerColor = palette.secondaryButtonBackground,
            contentColor = palette.secondaryButtonForeground,
            disabledContainerColor = palette.secondaryButtonBackground.copy(alpha = 0.5f),
            disabledContentColor = palette.secondaryButtonForeground.copy(alpha = 0.5f),
        ),
        elevation = ButtonDefaults.buttonElevation(
            defaultElevation = 0.dp,
            pressedElevation = 0.dp,
        ),
    ) {
        Text(text, fontWeight = FontWeight.Medium)
    }
}

// -- Agent Chat --

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun AgentChatScreen(manager: AppManager) {
    val state = manager.state
    val isDarkTheme = isSystemInDarkTheme()
    val palette = chatPalette(isDarkTheme)
    val density = LocalDensity.current
    var composeText by remember { mutableStateOf("") }
    val listState = rememberLazyListState()
    var headerBottomPx by remember { mutableIntStateOf(0) }
    var composeBarTopPx by remember { mutableIntStateOf(0) }

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
        val topContentPadding = with(density) {
            if (headerBottomPx == 0) 108.dp else headerBottomPx.toDp() + 16.dp
        }
        val bottomContentPadding = with(density) {
            if (composeBarTopPx == 0) {
                116.dp
            } else {
                (constraints.maxHeight - composeBarTopPx).coerceAtLeast(0).toDp() + 16.dp
            }
        }

        // Background radial gradient from top center (matches Figma)
        Box(
            modifier = Modifier
                .fillMaxSize()
                .background(palette.backgroundBase),
        )
        Box(
            modifier = Modifier
                .fillMaxSize()
                .background(
                    brush = Brush.radialGradient(
                        colors = palette.backgroundGlow,
                        center = androidx.compose.ui.geometry.Offset(w / 2f, 0f),
                        radius = h,
                    ),
                ),
        )

        // Messages list (full size, scrolls behind header and compose bar)
        LazyColumn(
            modifier = Modifier.fillMaxSize(),
            state = listState,
            contentPadding = PaddingValues(
                start = 16.dp, end = 16.dp,
                top = topContentPadding,
                bottom = bottomContentPadding,
            ),
            verticalArrangement = Arrangement.spacedBy(20.dp),
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
                MessageBubble(message, palette)
            }

            if (state.isAgentTyping) {
                item {
                    Text(
                        "Maple is typing...",
                        style = MaterialTheme.typography.bodySmall,
                        color = palette.metadataText,
                        modifier = Modifier.padding(start = 8.dp),
                    )
                }
            }
        }

        // Floating header islands
        Box(
            modifier = Modifier
                .fillMaxWidth()
                .statusBarsPadding()
                .padding(horizontal = 16.dp, vertical = 8.dp)
                .onGloballyPositioned { coordinates ->
                    headerBottomPx =
                        (coordinates.positionInRoot().y + coordinates.size.height).roundToInt()
                }
                .align(Alignment.TopCenter),
        ) {
            // MPL wordmark pill with chevron (centered)
            Box(
                modifier = Modifier
                    .align(Alignment.Center)
                    .then(
                        if (isDarkTheme) Modifier
                            .shadow(6.dp, RoundedCornerShape(99.dp))
                            .border(0.5.dp, palette.chromeBorder, RoundedCornerShape(99.dp))
                        else Modifier
                    )
                    .background(palette.chromeBackground, RoundedCornerShape(99.dp))
                    .padding(start = 16.dp, end = 12.dp, top = 12.dp, bottom = 12.dp),
            ) {
                Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    Icon(
                        painter = androidx.compose.ui.res.painterResource(id = cloud.opensecret.maple.R.drawable.maple_wordmark_abbr),
                        contentDescription = "Maple",
                        modifier = Modifier.height(16.dp),
                        tint = palette.headerWordmark,
                    )
                    Text("⌄", fontSize = 12.sp, fontWeight = FontWeight.Black, color = Pebble400)
                }
            }

            // Hamburger menu pill (start aligned)
            Box(
                modifier = Modifier
                    .align(Alignment.CenterStart)
                    .size(43.dp)
                    .then(
                        if (isDarkTheme) Modifier
                            .shadow(6.dp, RoundedCornerShape(99.dp))
                            .border(0.5.dp, palette.chromeBorder, RoundedCornerShape(99.dp))
                        else Modifier
                    )
                    .background(palette.chromeBackground, RoundedCornerShape(99.dp)),
                contentAlignment = Alignment.Center,
            ) {
                IconButton(
                    onClick = { manager.dispatch(AppAction.ToggleSettings) },
                    modifier = Modifier.size(43.dp),
                ) {
                    Icon(
                        imageVector = Icons.Filled.Menu,
                        contentDescription = "Menu",
                        tint = palette.secondaryIcon,
                        modifier = Modifier.size(18.dp),
                    )
                }
            }

            // Search pill (end aligned)
            Box(
                modifier = Modifier
                    .align(Alignment.CenterEnd)
                    .size(43.dp)
                    .then(
                        if (isDarkTheme) Modifier
                            .shadow(6.dp, RoundedCornerShape(99.dp))
                            .border(0.5.dp, palette.chromeBorder, RoundedCornerShape(99.dp))
                        else Modifier
                    )
                    .background(palette.chromeBackground, RoundedCornerShape(99.dp)),
                contentAlignment = Alignment.Center,
            ) {
                IconButton(
                    onClick = { },
                    modifier = Modifier.size(43.dp),
                ) {
                    Icon(
                        imageVector = Icons.Filled.Search,
                        contentDescription = "Search",
                        tint = palette.secondaryIcon,
                        modifier = Modifier.size(18.dp),
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
                .onGloballyPositioned { coordinates ->
                    composeBarTopPx = coordinates.positionInRoot().y.roundToInt()
                }
                .background(
                    if (isDarkTheme) palette.chromeBackground else Color.White.copy(alpha = 0.64f),
                    RoundedCornerShape(24.dp),
                )
                .then(
                    if (isDarkTheme) Modifier.border(0.5.dp, palette.chromeBorder, RoundedCornerShape(24.dp)) else Modifier
                )
                .padding(start = 16.dp, end = 16.dp, top = 12.dp, bottom = 14.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            val composeFocus = remember { FocusRequester() }

            Column(
                modifier = Modifier.weight(1f),
                verticalArrangement = Arrangement.spacedBy(4.dp),
            ) {
                BasicTextField(
                    value = composeText,
                    onValueChange = { composeText = it },
                    modifier = Modifier.fillMaxWidth().focusRequester(composeFocus),
                    textStyle = MaterialTheme.typography.bodyMedium.copy(
                        color = palette.composeText,
                        fontWeight = FontWeight.Medium,
                        fontSize = 15.sp,
                    ),
                    singleLine = true,
                    cursorBrush = SolidColor(Maple500),
                    keyboardOptions = KeyboardOptions(imeAction = ImeAction.Send),
                    keyboardActions = KeyboardActions(onSend = {
                        val text = composeText.trim()
                        if (text.isNotEmpty() && !state.isAgentTyping) {
                            manager.dispatch(AppAction.SendMessage(content = text))
                            composeText = ""
                        }
                    }),
                    decorationBox = { innerTextField ->
                        Box(modifier = Modifier.fillMaxWidth()) {
                            if (composeText.isEmpty()) {
                                Text("Write...", color = palette.composePlaceholder, fontSize = 15.sp, fontWeight = FontWeight.Medium)
                            }
                            innerTextField()
                        }
                    },
                )

                Box(
                    modifier = Modifier
                        .size(24.dp)
                        .background(Maple500.copy(alpha = 0.15f), CircleShape),
                    contentAlignment = Alignment.Center,
                ) {
                    Text("+", fontSize = 14.sp, fontWeight = FontWeight.Bold, color = Maple500)
                }
            }

            val canSend = composeText.trim().isNotEmpty() && !state.isAgentTyping

            Box(
                modifier = Modifier
                    .width(71.dp)
                    .clip(RoundedCornerShape(99.dp))
                    .background(
                        brush = Brush.verticalGradient(
                            colors = if (canSend) {
                                listOf(Maple500.copy(alpha = 0.5f), Color(0xFFE8633D).copy(alpha = 0.5f))
                            } else {
                                listOf(Maple500.copy(alpha = 0.5f), Color(0xFFE8633D).copy(alpha = 0.5f))
                            },
                        ),
                    )
                    .clickable(enabled = canSend) {
                        val text = composeText.trim()
                        if (text.isNotEmpty()) {
                            manager.dispatch(AppAction.SendMessage(content = text))
                            composeText = ""
                        }
                    }
                    .padding(horizontal = 24.dp, vertical = 8.dp),
                contentAlignment = Alignment.Center,
            ) {
                Text(
                    "↑",
                    fontSize = 20.sp,
                    fontWeight = FontWeight.Black,
                    color = Color.White.copy(alpha = if (canSend) 1f else 0.1f),
                )
            }
        }
    }

    if (state.showSettings) {
        SettingsSheet(manager = manager, palette = palette)
    }

    if (state.confirmDeleteAgent) {
        AlertDialog(
            onDismissRequest = { manager.dispatch(AppAction.CancelDeleteAgent) },
            containerColor = palette.sheetBackground,
            titleContentColor = palette.surfaceText,
            textContentColor = palette.metadataText,
            title = { Text("Delete Agent?") },
            text = { Text("This will permanently delete your agent conversation history. This cannot be undone.") },
            confirmButton = {
                TextButton(onClick = { manager.dispatch(AppAction.ConfirmDeleteAgent) }) {
                    Text("Delete", color = MapleError)
                }
            },
            dismissButton = {
                TextButton(onClick = { manager.dispatch(AppAction.CancelDeleteAgent) }) {
                    Text("Cancel", color = palette.surfaceText)
                }
            },
        )
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun SettingsSheet(manager: AppManager, palette: ChatPalette) {
    val sheetState = rememberModalBottomSheetState()

    ModalBottomSheet(
        onDismissRequest = { manager.dispatch(AppAction.ToggleSettings) },
        sheetState = sheetState,
        containerColor = palette.sheetBackground,
        contentColor = palette.surfaceText,
    ) {
        Column(modifier = Modifier.padding(horizontal = 16.dp, vertical = 8.dp)) {
            Text(
                "Settings",
                style = MaterialTheme.typography.titleMedium,
                color = palette.surfaceText,
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
            HorizontalDivider(modifier = Modifier.padding(vertical = 8.dp), color = palette.sheetDivider)
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
                    tint = palette.sheetSecondaryText,
                    modifier = Modifier.padding(end = 8.dp),
                )
                Text("Sign Out", color = palette.surfaceText)
            }
            Spacer(modifier = Modifier.height(32.dp))
        }
    }
}

@Composable
private fun MessageBubble(message: ChatMessage, palette: ChatPalette) {
    val isUser = message.isUser
    val alignment = if (isUser) Alignment.End else Alignment.Start
    val bubbleShape = RoundedCornerShape(
        topStart = 24.dp, topEnd = 24.dp,
        bottomStart = if (isUser) 24.dp else 4.dp,
        bottomEnd = if (isUser) 4.dp else 24.dp,
    )

    Column(
        modifier = Modifier.fillMaxWidth(),
        horizontalAlignment = alignment,
    ) {
        if (isUser) {
            Box(
                modifier = Modifier
                    .widthIn(max = 300.dp)
                    .background(palette.userBubbleColor, bubbleShape)
                    .padding(horizontal = 12.dp, vertical = 8.dp),
            ) {
                Text(
                    text = message.content,
                    color = palette.userText,
                    style = TextStyle(
                        fontFamily = ManropeFamily,
                        fontWeight = FontWeight.Medium,
                        fontSize = 16.sp,
                        lineHeight = 26.sp,
                    ),
                )
            }
        } else {
            Text(
                text = message.content,
                color = palette.assistantText,
                style = TextStyle(
                    fontFamily = ManropeFamily,
                    fontWeight = FontWeight.Medium,
                    fontSize = 16.sp,
                    lineHeight = 26.sp,
                ),
                modifier = Modifier.widthIn(max = 300.dp),
            )
        }
        if (message.showTimestamp) {
            Text(
                text = message.timestampDisplay,
                style = MaterialTheme.typography.labelSmall.copy(fontSize = 10.sp),
                color = palette.metadataText,
                modifier = Modifier.padding(start = 8.dp, end = 8.dp).padding(top = 2.dp),
            )
        }
    }
}
